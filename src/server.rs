use io_uring::IoUring;
use std::collections::HashMap;
use std::mem::size_of;
use std::net::TcpListener;
use std::os::unix::io::AsRawFd; // why not std::os::fd::AsRawFd

use crate::EventType;
use crate::{handle_request, push_accept_entry, push_poll_entry, push_recv_entry, push_send_entry};

// client socket
// https://www.man7.org/linux/man-pages/man2/accept.2.html
// "This structure is filled in with the address of the peer socket, as
// known to the communications layer."
struct ClientSocket {
    socket: libc::sockaddr,
    socket_len: libc::socklen_t,
}

impl ClientSocket {
    pub fn new() -> ClientSocket {
        let sockaddr = libc::sockaddr {
            sa_family: libc::AF_INET as libc::sa_family_t,
            sa_data: [0; 14],
        };
        let sockaddrlen = size_of::<libc::sockaddr_in>() as libc::socklen_t;

        ClientSocket {
            socket: sockaddr,
            socket_len: sockaddrlen,
        }
    }
}

pub struct Server {
    server_fd: i32, // maybe u32?
    io_uring: IoUring,

    // keep track of event per connection <fd, EventType>
    // TODO: consider asnother level of indirection, i.e using a smart pointer to pass on the user_data
    // instead of the fd.
    // RawFd == i32, https://doc.rust-lang.org/std/os/fd/type.RawFd.html
    fd_event: HashMap<i32, EventType>,

    // keep track of response per connection <fd, EventType>
    fd_response: HashMap<i32, String>,

    // client socket
    client_socket: ClientSocket,
}

impl Server {
    pub fn new(address: &str) -> Result<Server, std::io::Error> {
        // server fd
        let listener = TcpListener::bind(address)?;
        // file descriptor of the bound listening socket
        let server_fd = listener.as_raw_fd();
        log::debug!("Server listening socket: {}", server_fd);

        let io_uring = IoUring::new(8)?;
        let fd_event = HashMap::new();
        let fd_response = HashMap::new();

        let client_socket = ClientSocket::new();
        Ok(Server {
            server_fd,
            io_uring,
            fd_event,    // map
            fd_response, // map
            client_socket,
        })
    }

    // This is blocking
    pub fn run(&mut self) -> Result<(), std::io::Error> {
        let (submitter, mut sq, mut cq) = self.io_uring.split();

        // start by pushing server_fd in the user_data of the accept operation
        // and in the fd_event map
        // so that we can retrieve the event type on this fd when the an event is completed.
        // We push only the fd in the user data, and keep the all the rest in a hashmap.
        push_accept_entry(
            &mut sq,
            self.server_fd,
            &mut self.client_socket.socket,
            &mut self.client_socket.socket_len,
        )?;
        self.fd_event.insert(self.server_fd, EventType::Accept);
        let mut accept_on = false;

        loop {
            if accept_on {
                // accept blocks, i.e it moves to the cq only when a new connection is accepted.
                push_accept_entry(
                    &mut sq,
                    self.server_fd,
                    &mut self.client_socket.socket,
                    &mut self.client_socket.socket_len,
                )?;
                // we do not push into the fd_event map here, because we always accept on the same fd (server_fd).
                // This is pushed once (before the loop) in the map, and it does not change.
                // When a new connection is established, the new connection fd is indeed pushed in the
                // map on the completion of the accept.
                accept_on = false;
            }
            log::trace!("before sq: {} cq: {}", sq.len(), cq.len());
            log::trace!("-----submitting-----");
            // waits until at least 1 event completes, and accept event blocks until a connection is present
            // https://www.man7.org/linux/man-pages/man2/accept.2.html
            match submitter.submit_and_wait(1) {
                Ok(_) => (),
                Err(ref err) if err.raw_os_error() == Some(libc::EBUSY) => break,
                Err(err) => return Err(err.into()),
            }
            cq.sync();
            sq.sync();
            log::trace!("after  sq: {} cq: {}", sq.len(), cq.len());

            /*
            => Accepted new connection | new connection fd: 5, server fd: 3
            pushing poll, fd: 5
            pushing accept, fd: 3
            before sq: 2 cq: 0
            -----submitting-----
            after  sq: 0 cq: 1 // poll is completed, acccept blocks
            => Polled | event: 1, fd: 5
            */

            while let Some(cqe) = cq.next() {
                // this is the result of each operation
                // accept returns a new fd, recv and send return the size of the buffer
                let retval = cqe.result();

                // contains the fd that was pushed in user data.
                // Accept pushed the server_fd while the other operations
                // push and use the new fd that accept has returned.
                let fd = cqe.user_data() as i32;

                log::trace!("fd_event map: {:?}", self.fd_event);
                let event_type = &self.fd_event[&fd]; // i32
                match event_type {
                    EventType::Accept => {
                        log::debug!(
                            "=> Accepted new connection | new connection fd: {}, server fd: {}",
                            retval,
                            fd
                        );
                        //TODO: error handling depending on the retval fd.
                        // errno -l
                        // EBADF 9 Bad file descriptor

                        // accept another connection
                        // when this accept event is submitted, it will be removed from the cq and it will block
                        accept_on = true; // system crashes if false!

                        // use the new socket that accept returns to poll for a message there
                        self.fd_event.insert(retval, EventType::Poll); // TODO: refactor so that there is
                                                                       // one push interface, pushing both
                        push_poll_entry(&mut sq, retval)?;
                    }
                    EventType::Poll => {
                        log::debug!("=> Polled | event: {}, connected fd: {}", retval, fd);
                        let mut buf = vec![0u8; 2048].into_boxed_slice();
                        push_recv_entry(&mut sq, fd, &mut buf)?;
                        // The Recv EventType overwrites the Poll on the same fd
                        self.fd_event.insert(fd, EventType::Recv { buf });
                    }
                    EventType::Recv { buf } => {
                        log::debug!("=> Recving | size: {}, connected fd: {}", retval, fd);
                        if retval == 0 {
                            // shutdown connection on an empty body.
                            // [FIN, ACK] message, Len: 0
                            log::info!("=> shutdown connection");
                            // clean fd from the fd_event map, we are no longer processing events for
                            // this connection.
                            self.fd_event.remove(&fd);
                            unsafe {
                                libc::close(fd);
                            }
                        } else {
                            handle_request(fd, buf, &mut self.fd_response);
                            push_send_entry(&mut sq, fd, &self.fd_response[&fd])?;
                            // The Send EventType overwrites the Recv on the same fd
                            self.fd_event.insert(fd, EventType::Send);
                        }
                    }
                    EventType::Send => {
                        log::debug!("=> Send | size: {}, fd: {}", retval, fd);
                        // check if there is another message to read
                        // poll blocks if the fd is not ready
                        // https://www.man7.org/linux/man-pages/man2/poll.2.html
                        // If we did not poll, the next message would create a new connection
                        push_poll_entry(&mut sq, fd)?;
                        self.fd_event.insert(fd, EventType::Poll);
                    }
                }
            }
        }
        Ok(())
    }
}
