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
}

fn push_read_entry(sq: &mut SubmissionQueue, fd: types::Fd, buf: &mut [u8; 2048]) -> std::io::Result<()> {
    let read_e = opcode::Read::new(fd, buf.as_mut_ptr(), buf.len() as _)
        .build()
        .user_data(EventType::Write as _);

    // push in the sumbission queue
    unsafe {
        sq.push(&read_e).expect("submission queue is full");
    }
    sq.sync();
    Ok(())
}

fn push_accept_entry(sq: &mut SubmissionQueue, fd: types::Fd, sa: &mut libc::sockaddr, salen: &mut libc::socklen_t) -> std::io::Result<()> {
    let accept_e = opcode::Accept::new(fd, sa, salen)
        .build()
        .user_data(EventType::Read as _);

    // push in the sumbission queue
    unsafe {
        sq.push(&accept_e).expect("submission queue is full");
    }
    println!("PUSHED read");
    sq.sync();
    Ok(())
}

fn get_event_type(data: u64) -> Result<EventType, io::Error> {
    match data {
        0 => Ok(EventType::Accept),
        1 => Ok(EventType::Read),
        2 => Ok(EventType::Write),
        _ => Err(io::Error::new(io::ErrorKind::Other, "Error in getting the event type")),
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

    loop {
        println!("len before submit: sq={} cq= {}", sq.len(), cq.len());
        match submitter.submit_and_wait(1) {
            Ok(_) => (),
            Err(ref err) if err.raw_os_error() == Some(libc::EBUSY) => break, 
            Err(err) => return Err(err.into()), }
        cq.sync();
        sq.sync();
        println!("len after submit: sq={} cq= {}", sq.len(), cq.len());

        while let Some(cqe) = cq.next() {
            let retval = cqe.result();
            let event = cqe.user_data();
            println!("retval: {}", retval);
            println!("user data: {}", event);

            let event_type = get_event_type(event).unwrap();

            match event_type {
                EventType::Read => {

                    let fd = retval;
                    push_read_entry(&mut sq, types::Fd(fd), &mut buf)?;

                }
                EventType::Write => {
                    println!("buffer:  {}", String::from_utf8_lossy(&buf));
                }
                _ => {
                    println!("Event type got: {:?}", event_type);
                    return Err(io::Error::from_raw_os_error(22))
                }
            }
        }
    }

    Ok(())
}
