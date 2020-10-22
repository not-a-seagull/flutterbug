// MIT/Apache2 License

use super::{Connection, PendingRequestFlags};
use crate::{event::Event, util::cycled_zeroes};
use core::iter;
use tinyvec::TinyVec;

const DET_TYPE_MASK: u8 = 0x7f;
const TYPE_ERROR: u8 = 0;
const TYPE_REPLY: u8 = 1;

#[derive(Debug)]
pub(crate) struct PendingRequest {
    first_request: u64,
    last_request: u64,
    flags: PendingRequestFlags,
}

impl<Conn: Connection> super::Display<Conn> {
    // process a set of 32 bytes into the system
    #[inline]
    fn process_bytes(&mut self, mut bytes: TinyVec<[u8; 32]>) -> crate::Result {
        // get the sequence number
        let sequence = u16::from_ne_bytes([bytes[2], bytes[3]]);

        if bytes[0] == TYPE_REPLY {
            log::info!("Received bytes of type REPLY");
            // convert bytes to a boxed slice
            bytes.move_to_the_heap();
            let bytes = match bytes {
                TinyVec::Heap(h) => h.into_boxed_slice(),
                _ => unreachable!(),
            };

            self.pending_replies.insert(sequence as u64, bytes);
        } else if bytes[0] == TYPE_ERROR {
            // XCB has some convoluted machinery for errors
            // thank god Rust has better error handling
            return Err(crate::Error::from_x_error(bytes));
        } else {
            log::info!("Received bytes of type EVENT");
            // this is an event
            let event = Event::from_bytes(bytes)?;
            self.event_queue.push_back(event);
        }

        Ok(())
    }

    // add an entry to the pending elements linked list
    #[inline]
    pub(crate) fn expect_reply(&mut self, req: u64, flags: PendingRequestFlags) {
        let pereq = PendingRequest {
            first_request: req,
            last_request: req,
            flags,
        };
        self.pending_requests.push_back(pereq);
    }

    // wait for bytes to appear
    #[inline]
    pub fn wait(&mut self) -> crate::Result {
        log::debug!("Beginning wait...");

        // replies, errors, and events are all in units of 32 bytes
        let mut bytes: TinyVec<[u8; 32]> = cycled_zeroes(32);
        self.connection.read_packet(&mut bytes)?;
        log::debug!("Ending wait with 32 bytes");

        // in certain cases, we may have to read more bytes
        if let Some(ab) = additional_bytes(&bytes[..8]) {
            if ab != 0 {
                bytes.extend(iter::once(0).cycle().take(ab * 4));

                log::debug!("Waiting for {} additional bytes", ab * 4);
                self.connection.read_packet(&mut bytes[32..])?;
                log::debug!("Ending wait with {} additional bytes", ab * 4);
            }
        }

        self.process_bytes(bytes)
    }

    // wait for bytes to appear, async redox
    #[cfg(feature = "async")]
    #[inline]
    pub async fn wait_async(&mut self) -> crate::Result {
        // see above function for more information
        let mut bytes: TinyVec<[u8; 32]> = cycled_zeroes(32);
        self.connection.read_packet_async(&mut bytes).await?;

        if let Some(ab) = additional_bytes(&bytes[..8]) {
            bytes.extend(iter::once(0).cycle().take(ab * 4));
            self.connection.read_packet_async(&mut bytes[32..]).await?;
        }

        self.process_bytes(bytes)
    }
}

#[inline]
fn additional_bytes(bytes: &[u8]) -> Option<usize> {
    if bytes[0] == TYPE_REPLY {
        let mut len_bytes = [0; 4];
        len_bytes.copy_from_slice(&bytes[4..8]);
        Some(u32::from_ne_bytes(len_bytes) as usize)
    } else {
        None
    }
}
