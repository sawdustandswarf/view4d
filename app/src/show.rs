//! The viewer in a pane of termmd: a surface placed in the terminal's
//! output, frames drawn when something changes, input from the terminal.

use std::rc::Rc;
use std::time::{Duration, Instant};

use termmd_surface::{Connection, Decoder, Event, InputModes, Request};
use termmd_vulkan::dmabuf::{self, ARGB8888};
use termmd_vulkan::ffi::{VK_IMAGE_USAGE_SAMPLED_BIT, VK_IMAGE_USAGE_TRANSFER_DST_BIT};
use view4d_scene::Frame;

use crate::Result;
use crate::gpu::{self, Slot, Staging};
use crate::viewer::{Step, Viewer};
use crate::watch;

/// Where it goes without `--place`: beside the shell, half the screen.
pub const DEFAULT_PLACE: &str = "place=right w=50% h=50%";
/// Where `--idle` puts it: small, over the top right corner.
pub const IDLE_PLACE: &str = "fixed=top-right w=36 h=14";
/// The size asked for first, in pixels; termmd says the size it is shown at.
const FIRST_SIZE: (u32, u32) = (640, 480);
/// Buffers termmd shows in turn: one shown, one drawn into.
const BUFFERS: u32 = 2;

/// Show `viewer` in termmd, placed by `place`'s keys, until it quits or
/// termmd goes. `focus`: whether it takes the keyboard when it starts.
/// The scene is polled every [`watch::EVERY`], to follow its file or the
/// clock.
pub fn show(mut viewer: Viewer, place: &str, focus: bool) -> Result<()> {
    let mut conn = Connection::from_env().map_err(|e| format!("not in termmd ({e}); --png OUT.png draws a picture"))?;
    let surface = conn.create(FIRST_SIZE.0, FIRST_SIZE.1, focus)?.surface;
    termmd_surface::place(surface, place)?;
    conn.send(&Request::Modes { surface, modes: InputModes::app() }, None)?;
    let mut decoder = Decoder::new();

    let device = Rc::new(termmd_vulkan::open_default(None)?);
    let usage = VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    // Layouts termmd (on the same GPU) can sample.
    let mods = dmabuf::modifiers(&device, ARGB8888, usage | VK_IMAGE_USAGE_SAMPLED_BIT)?;
    let mut slots: Vec<Slot> = Vec::new();
    let mut staging: Option<Staging> = None;
    let mut frame = Frame::new(1, 1);
    let mut size = (0, 0);
    // A frame is on its way: wait for termmd's `frame` before the next.
    let mut in_flight = false;
    let mut dirty = true;
    let mut next_look = Instant::now() + watch::EVERY;

    loop {
        // Moving by itself: a frame whenever termmd will take one.
        dirty |= viewer.animates();
        let ready = dirty && !in_flight && size != (0, 0) && slots.iter().any(|s| !s.busy);
        let timeout = if ready { Duration::ZERO } else { next_look.saturating_duration_since(Instant::now()) };
        // Everything termmd has sent first, then draw once for all of it.
        let ev = match conn.next(Some(timeout)) {
            Ok(ev) => ev,
            // termmd went, and the pane with it.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        if let Some(ev) = ev {
            match ev {
                Event::Configure { width, height, .. } if (width, height) != size && width > 0 && height > 0 => {
                    for b in 0..slots.len() as u32 {
                        conn.send(&Request::Forget { surface, buffer: b }, None)?;
                    }
                    slots = (0..BUFFERS)
                        .map(|b| Slot::new(&device, &conn, surface, b, (width, height), &mods))
                        .collect::<Result<_>>()?;
                    staging = Some(Staging::new(&device, width as u64 * height as u64 * 4)?);
                    frame = Frame::new(width, height);
                    size = (width, height);
                    in_flight = false;
                    dirty = true;
                }
                Event::Frame { .. } => in_flight = false,
                Event::Release { buffer, .. } => {
                    if let Some(s) = slots.get_mut(buffer as usize) {
                        s.busy = false;
                    }
                }
                Event::Input { data, .. } => {
                    for input in decoder.feed(&data) {
                        match viewer.input(&input, size) {
                            Step::Quit => return quit(&mut conn, surface),
                            Step::Redraw => dirty = true,
                            Step::Same => {}
                        }
                    }
                }
                Event::Error { message } => eprintln!("view4d: termmd says: {message}"),
                _ => {}
            }
            continue;
        }
        if Instant::now() >= next_look {
            next_look = Instant::now() + watch::EVERY;
            dirty |= viewer.scene.poll();
        }
        if !ready {
            continue;
        }
        let b = slots.iter().position(|s| !s.busy).expect("ready: one is free");
        viewer.tick();
        viewer.draw(&mut frame);
        let staging = staging.as_mut().expect("made with the slots");
        staging.pixels(size).copy_from_slice(&frame.pixels);
        gpu::copy(&device, staging, &slots[b].image, size)?;
        slots[b].busy = true;
        conn.send(&Request::Present { surface, buffer: b as u32 }, None)?;
        in_flight = true;
        dirty = false;
    }
}

fn quit(conn: &mut Connection, surface: u32) -> Result<()> {
    conn.send(&Request::Destroy { surface }, None)?;
    Ok(())
}
