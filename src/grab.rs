use std::{
    ffi::CStr,
    io, mem,
    os::raw::{c_char, c_void},
    ptr,
    sync::mpsc::Receiver,
};

use tokio::sync::mpsc::UnboundedSender;
use x11::xlib;

use crate::{Event, Request};

const MODIFIERS: [(u32, &str); 8] = [
    (xlib::ShiftMask, "Shift"),
    (xlib::LockMask, "Lock"),
    (xlib::ControlMask, "Control"),
    (xlib::Mod1Mask, "Mod1"),
    (xlib::Mod2Mask, "Mod2"),
    (xlib::Mod3Mask, "Mod3"),
    (xlib::Mod4Mask, "Mod4"),
    (xlib::Mod5Mask, "Mod5"),
];

unsafe fn grabber(events: UnboundedSender<Event>, messages: Receiver<Request>) {
    let display = xlib::XOpenDisplay(ptr::null());
    let window = xlib::XDefaultRootWindow(display);

    xlib::XSelectInput(display, window, xlib::KeyPressMask | xlib::KeyReleaseMask);

    let im = xlib::XOpenIM(display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
    let ic = xlib::XCreateIC(
        im,
        xlib::XNInputStyle_0.as_ptr(),
        xlib::XIMPreeditNothing | xlib::XIMStatusNothing,
        xlib::XNClientWindow_0.as_ptr(),
        window,
        ptr::null_mut::<c_void>(),
    );
    xlib::XSetICFocus(ic);

    let mut display_fd = libc::pollfd {
        fd: xlib::XConnectionNumber(display),
        events: libc::POLLIN,
        revents: 0,
    };

    let process_event = |mut event: xlib::XEvent| {
        let filter = xlib::XFilterEvent(&mut event, window) == xlib::True;

        if event.get_type() == xlib::KeyPress {
            let mut buf = [0u8; 32];
            let mut status = 0;
            let mut keysym = 0;
            let len = xlib::Xutf8LookupString(
                ic,
                &mut event.key,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as i32,
                &mut keysym,
                &mut status,
            );

            let key = match xlib::XKeysymToString(keysym) {
                keysym if keysym.is_null() => String::new(),
                keysym => String::from_utf8_lossy(CStr::from_ptr(keysym).to_bytes()).to_string(),
            };

            let modifiers = MODIFIERS
                .into_iter()
                .filter(|(mask, _)| event.key.state & *mask != 0)
                .map(|(_, name)| name.to_string())
                .collect::<Vec<_>>();

            events
                .send(Event::Key {
                    key,
                    modifiers,
                    code: event.key.keycode,
                })
                .unwrap();

            if !filter && (status == xlib::XLookupChars || status == xlib::XLookupBoth) {
                events
                    .send(Event::Text {
                        value: String::from_utf8_lossy(&buf[..len as usize]).to_string(),
                    })
                    .unwrap();
            }
        }
    };

    let grab_keyboard = || {
        let status = xlib::XGrabKeyboard(
            display,
            window,
            xlib::True,
            xlib::GrabModeAsync,
            xlib::GrabModeAsync,
            xlib::CurrentTime,
        );
        if status != xlib::GrabSuccess && status != xlib::AlreadyGrabbed {
            eprintln!("XGrabKeyboard error: {}", status)
        }
    };

    let ungrab_keyboard = || {
        xlib::XUngrabKeyboard(display, xlib::CurrentTime);
    };

    loop {
        if let Ok(request) = messages.try_recv() {
            match request {
                Request::Grab { enabled } if enabled => grab_keyboard(),
                Request::Grab { enabled } if !enabled => ungrab_keyboard(),
                _ => (),
            }
        }

        display_fd.revents = 0;
        match libc::poll(&mut display_fd, 1, 1) {
            0 => (),
            -1 => {
                eprintln!("poll: {}", io::Error::last_os_error());
                break;
            }
            _ => {
                while xlib::XPending(display) > 0 {
                    let mut event: xlib::XEvent = mem::zeroed();
                    xlib::XNextEvent(display, &mut event);
                    process_event(event);
                }
            }
        }
    }

    xlib::XDestroyIC(ic);
    xlib::XCloseIM(im);
    xlib::XCloseDisplay(display);
}

pub fn start_grabber(events: UnboundedSender<Event>, messages: Receiver<Request>) {
    unsafe { grabber(events, messages) }
}
