use io_uring::{opcode, types, SubmissionQueue};
use log;
use std::collections::HashMap;
use std::fs;
//use std::mem::size_of;
//use std::net::TcpListener;
//use std::os::unix::io::AsRawFd;

pub mod server;

#[derive(Debug)]
enum EventType {
    Accept,
    Recv { buf: Box<[u8]> },
    Send,
    Poll,
}

fn push_poll_entry(sq: &mut SubmissionQueue, fd: i32) -> std::io::Result<()> {
    log::debug!("pushing poll at fd: {}", fd);
    let poll_e = opcode::PollAdd::new(types::Fd(fd), libc::POLLIN as _)
        .build()
        .user_data(fd.try_into().unwrap());

    unsafe {
        sq.push(&poll_e).expect("submission queue is full");
    }
    sq.sync();
    Ok(())
}

fn push_send_entry(sq: &mut SubmissionQueue, fd: i32, buf: &String) -> std::io::Result<()> {
    log::debug!("pushing send at fd: {}", fd);
    // send to the connected socket fd
    let write_e = opcode::Send::new(types::Fd(fd), buf.as_ptr(), buf.len() as _)
        .build()
        .user_data(fd.try_into().unwrap());

    unsafe {
        sq.push(&write_e).expect("submission queue is full");
    }
    sq.sync();
    Ok(())
}

fn push_recv_entry(sq: &mut SubmissionQueue, fd: i32, buf: &mut Box<[u8]>) -> std::io::Result<()> {
    log::debug!("pushing recv at fd: {}", fd);
    let read_e = opcode::Recv::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
        .build()
        .user_data(fd.try_into().unwrap());

    unsafe {
        sq.push(&read_e).expect("submission queue is full");
    }
    sq.sync();
    Ok(())
}

fn push_accept_entry(
    sq: &mut SubmissionQueue,
    fd: i32,
    sa: &mut libc::sockaddr,
    salen: &mut libc::socklen_t,
) -> std::io::Result<()> {
    log::debug!("pushing accept at fd: {}", fd);
    let accept_e = opcode::Accept::new(types::Fd(fd), sa, salen)
        .build()
        .user_data(fd.try_into().unwrap()); //u64

    unsafe {
        sq.push(&accept_e).expect("submission queue is full");
    }
    sq.sync();
    Ok(())
}

// we store the response per fd (client socket) in a hashmap
fn handle_request(fd: i32, buf: &Box<[u8]>, fd_resp_map: &mut HashMap<i32, String>) {
    let get = b"GET / HTTP/1.1\r\n";
    let (status_line, filename) = if buf.starts_with(get) {
        ("HTTP/1.1 200 OK", "resources/hello.html")
    } else {
        ("HTTP/1.1 404 NOT FOUND", "resources/404.html")
    };

    let contents = fs::read_to_string(filename).unwrap();
    let response = format!(
        "{}\r\nContent-Length: {}\r\n\r\n{}",
        status_line,
        contents.len(),
        contents
    );
    fd_resp_map.insert(fd, response);
}

/*
// Todo: Implement a Server struct and return this
// This function is blocking
pub fn run(listener: TcpListener) -> Result<(), std::io::Error> {
    // server file descriptor of the bound listening socket
    let server_fd = listener.as_raw_fd();
    log::debug!("Server socket: {}", server_fd);

    // client socket address structure
    // https://www.man7.org/linux/man-pages/man2/accept.2.html
    // "This structure is filled in with the address of the peer socket, as
    // known to the communications layer."
    let mut sockaddr = libc::sockaddr {
        sa_family: libc::AF_INET as libc::sa_family_t,
        sa_data: [0; 14],
    };
    let mut sockaddrlen = size_of::<libc::sockaddr_in>() as libc::socklen_t;

    let mut ring = IoUring::new(8)?;
    let (submitter, mut sq, mut cq) = ring.split();

    // internal housekeeping
    // keep track of event per connection <fd, EventType>
    // TODO: consider asnother level of indirection, i.e using a pointer to pass on the user_data
    // instead of the fd
    let mut fd_event = HashMap::new();
    // keep track of response per connection <fd, EventType>
    let mut fd_response = HashMap::new();

    // start by pushing server_fd in the user_data of the accept operation
    // and in the fd_event map
    // so that we can retrieve the event type on this fd when the an event is completed.
    // We push only the fd in the user data, and keep the all the rest in a hashmap.
    push_accept_entry(&mut sq, server_fd, &mut sockaddr, &mut sockaddrlen)?;
    fd_event.insert(server_fd, EventType::Accept);
    let mut accept_on = false;

    loop {
        if accept_on {
            // accept blocks, i.e it moves to the cq only when a new connection is accepted.
            push_accept_entry(&mut sq, server_fd, &mut sockaddr, &mut sockaddrlen)?;
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

            log::trace!("fd_event map: {:?}", fd_event);
            let event_type = &fd_event[&fd]; // i32
            match event_type {
                EventType::Accept => {
                    log::debug!(
                        "=> Accepted new connection | new connection fd: {}, server fd: {}",
                        retval,
                        fd
                    );
                    // accept another connection
                    // when this accept event is submitted, it will be removed from the cq and it will block
                    accept_on = true; // system crashes if false!

                    // use the new socket that accept returns to poll for a message there
                    fd_event.insert(retval, EventType::Poll); // TODO: refactor so that there is
                                                              // one push interface, pushing both
                    push_poll_entry(&mut sq, retval)?;
                }
                EventType::Poll => {
                    log::debug!("=> Polled | event: {}, connected fd: {}", retval, fd);
                    let mut buf = vec![0u8; 2048].into_boxed_slice();
                    push_recv_entry(&mut sq, fd, &mut buf)?;
                    // The Recv EventType overwrites the Poll on the same fd
                    fd_event.insert(fd, EventType::Recv { buf });
                }
                EventType::Recv { buf } => {
                    log::debug!("=> Recving | size: {}, connected fd: {}", retval, fd);
                    if retval == 0 {
                        // shutdown connection on an empty body.
                        // [FIN, ACK] message, Len: 0
                        log::info!("=> shutdown connection");
                        // clean fd from the fd_event map, we are no longer processing events for
                        // this connection.
                        fd_event.remove(&fd);
                        unsafe {
                            libc::close(fd);
                        }
                    } else {
                        handle_request(fd, buf, &mut fd_response);
                        push_send_entry(&mut sq, fd, &fd_response[&fd])?;
                        // The Send EventType overwrites the Recv on the same fd
                        fd_event.insert(fd, EventType::Send);
                    }
                }
                EventType::Send => {
                    log::debug!("=> Send | size: {}, fd: {}", retval, fd);
                    // check if there is another message to read
                    // poll blocks if the fd is not ready
                    // https://www.man7.org/linux/man-pages/man2/poll.2.html
                    // If we did not poll, the next message would create a new connection
                    push_poll_entry(&mut sq, fd)?;
                    fd_event.insert(fd, EventType::Poll);
                }
            }
        }
    }
    Ok(())
}
*/
