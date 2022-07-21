use std::mem::size_of;
use std::{net::TcpListener, os::unix::prelude::AsRawFd};
use io_uring::{opcode, types, IoUring, SubmissionQueue};
use std::io;

//use std::{thread, time};

#[derive(Debug)]
enum EventType {
    Accept, //0
    Read,  //1
    Write, //2
    Poll, //3
}

struct UserData {
    event: EventType,
    ret_socket: types::Fd,
}

/*
   hashmap<u64, UserData>
   0, USerData
   1, UserData
   2, UserData

 */

fn push_poll_entry(sq: &mut SubmissionQueue, fd: types::Fd) -> std::io::Result<()> {
    let poll_e = opcode::PollAdd::new(fd, libc::POLLIN as _)
        .build()
        .user_data(EventType::Poll as _);

    // push in the sumbission queue
    unsafe {
        sq.push(&poll_e).expect("submission queue is full");
    }
    println!("PUSHED poll");
    sq.sync();
    Ok(())
}

fn push_write_entry(sq: &mut SubmissionQueue, fd: types::Fd, buf: &mut [u8; 2048]) -> std::io::Result<()> {
    let write_e = opcode::Write::new(fd, buf.as_mut_ptr(), buf.len() as _)
        .build()
        .user_data(EventType::Write as _);

    // push in the sumbission queue
    unsafe {
        sq.push(&write_e).expect("submission queue is full");
    }
    println!("PUSHED write");
    sq.sync();
    Ok(())
}

fn push_read_entry(sq: &mut SubmissionQueue, fd: types::Fd, buf: &mut [u8; 2048]) -> std::io::Result<()> {
    let read_e = opcode::Read::new(fd, buf.as_mut_ptr(), buf.len() as _)
        .build()
        .user_data(EventType::Read as _);

    // push in the sumbission queue
    unsafe {
        sq.push(&read_e).expect("submission queue is full");
    }
    println!("PUSHED read");
    sq.sync();
    Ok(())
}

fn push_accept_entry(sq: &mut SubmissionQueue, fd: types::Fd, sa: &mut libc::sockaddr, salen: &mut libc::socklen_t) -> std::io::Result<()> {
    let accept_e = opcode::Accept::new(fd, sa, salen)
        .build()
        .user_data(EventType::Accept as _);

    // push in the sumbission queue
    unsafe {
        sq.push(&accept_e).expect("submission queue is full");
    }
    println!("PUSHED accept");
    sq.sync();
    Ok(())
}

// push user data as u64 (_), get back EventType to use in match
fn get_event_type(data: u64) -> Result<EventType, io::Error> {
    match data {
        0 => Ok(EventType::Accept),
        1 => Ok(EventType::Read),
        2 => Ok(EventType::Write),
        3 => Ok(EventType::Poll),
        _ => Err(io::Error::new(io::ErrorKind::Other, "Error getting the event type")),
    }
}

fn main() -> std::io::Result<()> {

    let mut buf = [0u8; 2048];

    let listener = TcpListener::bind("0.0.0.0:8090").unwrap();
    println!("listening on {}", listener.local_addr()?);
    let fd = types::Fd(listener.as_raw_fd());

    let mut ring = IoUring::new(8)?;
    let (submitter, mut sq, mut cq) = ring.split();

    // client socket address
    let mut sockaddr = libc::sockaddr {
        sa_family: libc::AF_INET as libc::sa_family_t,
        sa_data: [0; 14] };
    let mut sockaddrlen = size_of::<libc::sockaddr_in>() as libc::socklen_t;

    push_accept_entry(&mut sq, fd, &mut sockaddr, &mut sockaddrlen)?;
    println!("push entry done");

    // fd returned by accept
    let mut new_socket = 0;
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
            let retval = cqe.result();
            let event = cqe.user_data();
            println!("retval: {}", retval);
            println!("user data: {}", event);

            let event_type = get_event_type(event).unwrap();

            match event_type {
                EventType::Accept => {
                    println!("Accept cqe data");
                    // raise flag to push accept
                    accept_on = true;
                    // use the new socket that accept returns
                    new_socket = retval;
                    println!("new socket after accept: {}", new_socket);
                    // no need to poll, we can read immediately
                    push_poll_entry(&mut sq, types::Fd(new_socket))?;
                    //push_read_entry(&mut sq, types::Fd(new_socket), &mut buf)?;
                }
                EventType::Poll => {
                    println!("Poll cqe data");
                    push_read_entry(&mut sq, types::Fd(new_socket), &mut buf)?;

                }
                EventType::Read => {
                    println!("Read cqe data");
                    // get read buffer and handle request
                    // push write entry
                    println!("new socket after read: {}", new_socket);
                    push_write_entry(&mut sq, types::Fd(new_socket), &mut buf)?;

                }
                EventType::Write => {
                    println!("buffer:  {}", String::from_utf8_lossy(&buf));
                    // check if there is another message to read
                    push_poll_entry(&mut sq, types::Fd(new_socket))?;
                }
                /*
                _ => {
                    println!("Event type got: {:?}", event_type);
                    return Err(io::Error::from_raw_os_error(22))
                }
                */
            }
        }
    }

    Ok(())
}
