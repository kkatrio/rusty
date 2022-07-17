use std::mem::size_of;
use std::{net::TcpListener, os::unix::prelude::AsRawFd};
use io_uring::{opcode, types, IoUring, SubmissionQueue};

//enum EventType {
//    Accept,
//    Read,
//    Write,
//}

fn push_accept_entry(sq: &mut SubmissionQueue, fd: types::Fd, sa: &mut libc::sockaddr, salen: &mut libc::socklen_t) -> std::io::Result<()> {
    let accept_e = opcode::Accept::new(fd, sa, salen)
        .build()
        .user_data(65);

    // push in the sumbission queue
    unsafe {
        sq.push(&accept_e).expect("submission queue is full");
    }
    sq.sync();
    Ok(())
}

fn main() -> std::io::Result<()> {

    let listener = TcpListener::bind("0.0.0.0:8090").unwrap();
    println!("listening on {}", listener.local_addr()?);
    let fd = types::Fd(listener.as_raw_fd());

    let mut ring = IoUring::new(8)?;
    let (submitter, mut sq, mut cq) = ring.split();

    // client socket address
    let mut sockaddr = libc::sockaddr {
        sa_family: libc::AF_INET as libc::sa_family_t,
        sa_data: [0; 14]
    };
    let mut sockaddrlen = size_of::<libc::sockaddr_in>() as libc::socklen_t;

    push_accept_entry(&mut sq, fd, &mut sockaddr, &mut sockaddrlen)?;
    println!("push entry done");
    submitter.submit_and_wait(1)?;
    println!("submit done");

    while let Some(cqe) = cq.next() {
        let retval = cqe.result();
        let event = cqe.user_data();
        println!("retval: {}", retval);
        println!("user data: {}", event);
    }

    Ok(())
}
