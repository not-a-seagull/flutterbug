// MIT/Apache2 License
// This file is largely inspired by x11rb

#![cfg(all(feature = "std", unix))]

use crate::{utils::convert_nix_error, Fd};
use alloc::vec::Vec;
use nix::sys::{
    socket::{recvmsg, sendmsg, ControlMessage, ControlMessageOwned, MsgFlags},
    uio::IoVec,
};
use std::{
    io::{Error, Write},
    os::unix::io::{AsRawFd, RawFd},
};

#[cfg(feature = "async")]
use async_io::Async;
#[cfg(feature = "async")]
use core::{
    future::{Context, Future, Poll},
    pin::Pin,
};
#[cfg(feature = "async")]
use futures_io::{AsyncRead, AsyncWrite};

#[inline]
fn send_msg_packet(conn: RawFd, data: &[u8], fds: &mut Vec<Fd>) -> io::Result<()> {
    #[inline]
    fn sendmsg_loop(conn: RawFd, data: &[u8], cmsgs: &[ControlMessage<'_>]) -> io::Result<()> {
        let data = IoVec::from_slice(data);
        loop {
            match sendmsg(conn, data, cmsgs, MsgFlags::empty(), None) {
                Ok(n) => return Ok(n),
                Err(nix::Error::Sys(nix::errno::Errno::EINTR)) => (),
                Err(e) => return Err(convert_nix_error(e)),
            }
        }
    }

    let res = if fds.is_empty() {
        sendmsg_loop(connfd, data, &[])
    } else {
        let cmsgs = [ControlMessage::ScmRights(&fds[..])];
        sendmsg_loop(connfd, data, &cmsgs)
    };

    if res.is_ok() {
        fds.clear();
    }

    res
}

/// For Unix stream types, we can use this function to send FDs.
#[inline]
pub fn send_packet_unix<Conn: AsRawFd + Write>(
    conn: &mut Conn,
    data: &[u8],
    fds: &mut Vec<Fd>,
) -> crate::Result {
    let connfd = conn.as_raw_fd();

    send_msg_packet(connfd, data, fds)?;
    Ok(())
}

/// The same as the above function, but async.
#[inline]
pub async fn send_packet_unix_async<Conn: AsRawFd + Write + Unpin>(
    conn: Arc<Async<Conn>>,
    data: &[u8],
    fds: &mut Vec<Fd>,
) -> crate::Result {
    // TODO: make sure this isn't unsound. the way we use it, it shouldn't be
    conn.get_mut()
        .expect("Failed to get mutable reference to connection")
        .write_with(|conn| {
            let connfd = conn.as_raw_fd();
            send_msg_packet(connfd, data, fds)
        })
        .await?;

    Ok(())
}

/// Read a packet, unix style. Includes fds.
#[inline]
fn read_msg_packet(conn: RawFd, data: &mut [u8], fds: &mut Vec<Fd>) -> io::Result<()> {
    const MAX_FDS: usize = 16;
    let mut cmsg = nix::cmsg_space!([Fd; MAX_FDS]);
    let data = [IoVec::from_mut_slice(data)];

    let msg = loop {
        match recvmsg(fd, &data[..], Some(&mut cmsg), MsgFlags::empty()) {
            Ok(m) => break m,
            Err(nix::Error::Sys(nix::errno::Errno::EINTR)) => (),
            Err(e) => return Err(convert_nix_error(e)),
        }
    };

    fds.extend(msg.cmsgs().flat_map(|cmsg| match cmsg {
        ControlMessageOwned::ScmRights(r) => r,
        _ => vec![],
    }));

    Ok(())
}

/// Read a packet, unix style.
#[inline]
pub fn read_packet_unix<Conn: AsRawFd + Read>(
    conn: &mut Conn,
    data: &mut [u8],
    fds: &mut Vec<Fd>,
) -> crate::Result {
    let connfd = conn.as_raw_fd();
    read_msg_packet(connfd, data, fds)?;
    Ok(())
}

/// Read a packet, async redox.
#[inline]
pub async fn read_packet_unix_async<Conn: AsRawFd + Read + Unpin>(
    conn: Arc<Async<Conn>>,
    data: &mut [u8],
    fds: &mut Vec<Fd>,
) -> crate::Result {
    // TODO: same as above
    conn.read_with(|conn| {
        let connfd = conn.as_raw_fd();
        read_msg_packet(connfd, data, fds)
    })
    .await?;

    Ok(())
}