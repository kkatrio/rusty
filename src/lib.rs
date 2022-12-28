use io_uring::{opcode, types, SubmissionQueue};
use log;

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
