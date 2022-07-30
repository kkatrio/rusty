use std::mem::size_of;
use std::{net::TcpListener, os::unix::prelude::AsRawFd};
use io_uring::{opcode, types, IoUring, SubmissionQueue};
use std::collections::HashMap;
use std::fs;

#[derive(Debug)]
enum EventType {
    Accept,
    Read {
        buf: Box<[u8]>,
    },
    Write,
    Poll,
}

fn push_poll_entry(sq: &mut SubmissionQueue, fd: i32) -> std::io::Result<()> {
    let poll_e = opcode::PollAdd::new(types::Fd(fd), libc::POLLIN as _)
        .build()
        .user_data(fd.try_into().unwrap());

    unsafe {
        sq.push(&poll_e).expect("submission queue is full");
    }
    sq.sync();
    Ok(())
}

fn push_write_entry(sq: &mut SubmissionQueue, fd: i32, buf: &String) -> std::io::Result<()> {
    let write_e = opcode::Send::new(types::Fd(fd), buf.as_ptr(), buf.len() as _)
        .build()
        .user_data(fd.try_into().unwrap());

    unsafe {
        sq.push(&write_e).expect("submission queue is full");
    }
    sq.sync();
    Ok(())
}

fn push_read_entry(sq: &mut SubmissionQueue, fd: i32, buf: &mut Box<[u8]>) -> std::io::Result<()> {
    let read_e = opcode::Recv::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
        .build()
        .user_data(fd.try_into().unwrap());

    unsafe {
        sq.push(&read_e).expect("submission queue is full");
    }
    sq.sync();
    Ok(())
}

fn push_accept_entry(sq: &mut SubmissionQueue, fd: i32,  sa: &mut libc::sockaddr, salen: &mut libc::socklen_t) -> std::io::Result<()> {
    println!("pushing accept, fd: {}", fd);
    let accept_e = opcode::Accept::new(types::Fd(fd), sa, salen)
        .build()
        .user_data(fd.try_into().unwrap()); //u64

    unsafe {
        sq.push(&accept_e).expect("submission queue is full");
    }
    sq.sync();
    Ok(())
}

fn handle_request(fd: i32, _buf: &Box<[u8]>, fd_resp_map: &mut HashMap<i32, String>) {

    let status_line = "HTTP/1.1 200 OK";
    let contents = fs::read_to_string("resources/hello.html").unwrap();
    let response = format!(
        "{}\r\nContent-Length: {}\r\n\r\n{}",
        status_line,
        contents.len(),
        contents
    );

    fd_resp_map.insert(fd, response);
}

fn main() -> std::io::Result<()> {

    let listener = TcpListener::bind("0.0.0.0:8090").unwrap();
    println!("listening on {}", listener.local_addr()?);
    let server_fd = listener.as_raw_fd();
    println!("Server socket: {}", server_fd);

    let mut ring = IoUring::new(8)?;
    let (submitter, mut sq, mut cq) = ring.split();

    // client socket address
    let mut sockaddr = libc::sockaddr {
        sa_family: libc::AF_INET as libc::sa_family_t,
        sa_data: [0; 14] };
    let mut sockaddrlen = size_of::<libc::sockaddr_in>() as libc::socklen_t;

    // keep track of event per connection <fd, EventType>
    let mut fd_event = HashMap::new();
    // keep track of response per connection <fd, EventType>
    let mut fd_response = HashMap::new();

    push_accept_entry(&mut sq, server_fd, &mut sockaddr, &mut sockaddrlen)?;
    fd_event.insert(server_fd, EventType::Accept);
    let mut accept_on = false;

    loop {
        match submitter.submit_and_wait(1) {
            Ok(_) => (),
            Err(ref err) if err.raw_os_error() == Some(libc::EBUSY) => break, 
            Err(err) => return Err(err.into()), }
        cq.sync();
        sq.sync();

        if accept_on {
            push_accept_entry(&mut sq, server_fd, &mut sockaddr, &mut sockaddrlen)?;
            accept_on = false;
        }

        while let Some(cqe) = cq.next() {

            // accept returns a new fd, recv and send return the size of the buffer
            let retval = cqe.result();

            // contains the fd that was pushed in user data.
            // Accept pushed the server_fd while the other operations
            // push and use the second fd that accept has returned.
            let fd = cqe.user_data() as i32;

            let event_type = &fd_event[&fd]; // i32
            match event_type {
                EventType::Accept => {
                    println!("Accept new connection | retval: {}, fd(server socket): {}", retval, fd);
                    // accept another connection
                    accept_on = true;
                    // use the new socket that accept returns
                    fd_event.insert(retval, EventType::Poll);
                    push_poll_entry(&mut sq, retval)?;
                }
                EventType::Poll => {
                    println!("Poll | retval: {}, fd: {}", retval, fd);
                    let mut buf = vec![0u8; 2048].into_boxed_slice();
                    push_read_entry(&mut sq, fd, &mut buf)?;
                    fd_event.insert(fd, EventType::Read {buf});

                }
                EventType::Read {buf} => {
                    println!("Read | retval: {}, fd: {}", retval, fd);
                    if retval == 0 {
                        println!("shutdown");
                        unsafe {
                            libc::close(fd);
                        }
                    }
                    else {
                        handle_request(fd, buf, &mut fd_response);
                        push_write_entry(&mut sq, fd, &fd_response[&fd])?;
                        fd_event.insert(fd, EventType::Write);
                    }
                }
                EventType::Write => {
                    println!("Write | retval: {}, fd: {}", retval, fd);
                    // check if there is another message to read
                    push_poll_entry(&mut sq, fd)?;
                    fd_event.insert(fd, EventType::Poll);
                }
            }
        }
    }

    Ok(())
}
