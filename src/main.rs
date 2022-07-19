use std::mem::size_of;
use std::{net::TcpListener, os::unix::prelude::AsRawFd};
use io_uring::{opcode, types, IoUring, SubmissionQueue};
use std::io;

//use std::{thread, time};

//enum EventType {
//    Accept, 65
//    Read, 66
//    Write, 67
//}

//fn push_read_entry(sq: &mut SubmissionQueue, fd: types::Fd, buf: &mut Box<[u8]>) -> std::io::Result<()> {
//    let read_e = opcode::Recv::new(fd, buf.as_mut_ptr(), buf.len().try_into().unwrap())
//        .build()
//        .user_data(70);
//
//    // push in the sumbission queue
//    unsafe {
//        sq.push(&read_e).expect("submission queue is full");
//    }
//    sq.sync();
//    Ok(())
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
            Err(err) => return Err(err.into()),
        }
        cq.sync();
        sq.sync();
        println!("len after submit: sq={} cq= {}", sq.len(), cq.len());

        while let Some(cqe) = cq.next() {
            let retval = cqe.result();
            let event = cqe.user_data();
            println!("retval: {}", retval);
            println!("user data: {}", event);

            match event {
                65 => {
                    println!("cqe accept 65");
                    //Accept
                    // push a Read operation
                    //let mut buf = vec![0u8].into_boxed_slice();
                    //push_read_entry(&mut sq, fd, &mut buf)?;

                    let fd = retval;
                    let read_e = opcode::Read::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
                    .build()
                    .user_data(70);

                    // push in the sumbission queue
                    unsafe {
                        sq.push(&read_e).expect("submission queue is full");
                    }
                    sq.sync();
                    println!("PUSHED read");

                }
                70 => {
                    println!("cqe write 70");
                    println!("buffer:  {}", String::from_utf8_lossy(&buf));
                }
                _ => return Err(io::Error::from_raw_os_error(22))
            }


        }
    }



    Ok(())
}
