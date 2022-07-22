use std::mem::size_of;
use std::{net::TcpListener, os::unix::prelude::AsRawFd};
use io_uring::{opcode, types, IoUring, SubmissionQueue};
use std::collections::HashMap;

#[derive(Debug)]
enum EventType {
    Accept,
    Read,
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
    println!("PUSHED poll");
    sq.sync();
    Ok(())
}

fn push_write_entry(sq: &mut SubmissionQueue, fd: i32, buf: &mut [u8; 2048]) -> std::io::Result<()> {
    let write_e = opcode::Write::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
        .build()
        .user_data(fd.try_into().unwrap());

    unsafe {
        sq.push(&write_e).expect("submission queue is full");
    }
    println!("PUSHED write");
    sq.sync();
    Ok(())
}

fn push_read_entry(sq: &mut SubmissionQueue, fd: i32, buf: &mut [u8; 2048]) -> std::io::Result<()> {
    let read_e = opcode::Read::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
        .build()
        .user_data(fd.try_into().unwrap());

    unsafe {
        sq.push(&read_e).expect("submission queue is full");
    }
    println!("PUSHED read");
    sq.sync();
    Ok(())
}

fn push_accept_entry(sq: &mut SubmissionQueue, fd: i32,  sa: &mut libc::sockaddr, salen: &mut libc::socklen_t) -> std::io::Result<()> {
    let accept_e = opcode::Accept::new(types::Fd(fd), sa, salen)
        .build()
        .user_data(fd.try_into().unwrap()); //u64

    unsafe {
        sq.push(&accept_e).expect("submission queue is full");
    }
    println!("PUSHED accept");
    sq.sync();
    Ok(())
}

fn main() -> std::io::Result<()> {

    let mut buf = [0u8; 2048];

    let listener = TcpListener::bind("0.0.0.0:8090").unwrap();
    println!("listening on {}", listener.local_addr()?);
    let fd = listener.as_raw_fd();

    let mut ring = IoUring::new(8)?;
    let (submitter, mut sq, mut cq) = ring.split();

    // client socket address
    let mut sockaddr = libc::sockaddr {
        sa_family: libc::AF_INET as libc::sa_family_t,
        sa_data: [0; 14] };
    let mut sockaddrlen = size_of::<libc::sockaddr_in>() as libc::socklen_t;

    // keep track of event per connection
    let mut fd_event = HashMap::new();

    push_accept_entry(&mut sq, fd, &mut sockaddr, &mut sockaddrlen)?;
    fd_event.insert(fd, EventType::Accept);

    let mut accept_on = false;

    loop {
        println!("len before submit: sq={} cq= {}", sq.len(), cq.len());
        match submitter.submit_and_wait(1) {
            Ok(_) => (),
            Err(ref err) if err.raw_os_error() == Some(libc::EBUSY) => break, 
            Err(err) => return Err(err.into()), }
        cq.sync();
        sq.sync();
        println!("len after submit: sq={} cq= {}", sq.len(), cq.len());

        if accept_on {
            push_accept_entry(&mut sq, fd, &mut sockaddr, &mut sockaddrlen)?;
            accept_on = false;
        }

        while let Some(cqe) = cq.next() {

            // accept returns a new socket here
            let retval = cqe.result();

            // contains the fd that was pushed as user data
            let conn_fd = cqe.user_data() as i32;

            println!("retval: {}", retval);
            println!("conn_fd: {}", conn_fd);

            // what event completed on this socket
            let event_type = &fd_event[&conn_fd]; // i32
            println!("event_type: {:?}", event_type);

            match event_type {
                EventType::Accept => {
                    // raise flag to push another accept
                    accept_on = true;
                    // use the new socket that accept returns
                    fd_event.insert(retval, EventType::Poll);
                    push_poll_entry(&mut sq, retval)?;
                }
                EventType::Poll => {
                    push_read_entry(&mut sq, conn_fd, &mut buf)?;
                    fd_event.insert(conn_fd, EventType::Read);

                }
                EventType::Read => {
                    println!("conn_fd in Read: {}", conn_fd);
                    push_write_entry(&mut sq, conn_fd, &mut buf)?;
                    fd_event.insert(conn_fd, EventType::Write);

                }
                EventType::Write => {
                    println!("buffer:  {}", String::from_utf8_lossy(&buf));
                    // check if there is another message to read
                    push_poll_entry(&mut sq, conn_fd)?;
                    fd_event.insert(conn_fd, EventType::Poll);
                }
            }
        }
    }

    Ok(())
}
