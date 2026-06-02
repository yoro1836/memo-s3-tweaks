mod config;
mod core;
mod pipeline;
mod plugin;

use config::Config;
use core::*;
use pipeline::{Event, Pipeline, Side};
use mlua::Lua;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let mut config_path = "/data/user_de/0/com.android.shell/axeron/plugins/keyforge/keyforge.conf".to_string();
    let args: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" && i + 1 < args.len() { i += 1; config_path = args[i].clone(); }
        i += 1;
    }
    let config_path = Path::new(&config_path);
    let mut cfg = Config::load(config_path);
    let lua: &'static Lua = Box::leak(Box::new(Lua::new()));
    let mut pipeline = Pipeline::new();
    let _manifest = plugin::load_plugins(lua, &cfg.plugin_dir, &mut pipeline, &cfg.values);

    let ifd = unsafe { inotify_init1(IN_NONBLOCK) };
    if ifd < 0 { eprintln!("keyforge: inotify_init1 failed"); std::process::exit(1); }
    unsafe { let cpath = std::ffi::CString::new(INPUT_DIR).unwrap(); inotify_add_watch(ifd, cpath.as_ptr(), IN_CREATE | IN_DELETE); }

    let mut dev = Device::new();
    let ev_size = std::mem::size_of::<InputEvent>();
    let mut pending_releases: Vec<(std::time::Instant, pipeline::EmitEvent)> = Vec::new();
    let epfd = unsafe { epoll_create1(0) };
    if epfd < 0 { eprintln!("keyforge: epoll_create1 failed"); std::process::exit(1); }
    let mut ev_if = EpollEvent { events: EPOLLIN, data: ifd as u64 };
    unsafe { epoll_ctl(epfd, EPOLL_CTL_ADD, ifd, &mut ev_if); }
    let mut ev_fd = EpollEvent { events: EPOLLIN, data: 0 };
    let mut have_fd = false;

    loop {
        let fresh = Config::load(config_path);
        if fresh.vid != cfg.vid || fresh.pid != cfg.pid || fresh.plugin_dir != cfg.plugin_dir || fresh.values != cfg.values {
            pipeline = Pipeline::new();
            let _ = plugin::load_plugins(lua, &fresh.plugin_dir, &mut pipeline, &fresh.values);
            cfg = fresh;
            if dev.fd >= 0 { dev.deinit(); have_fd = false; }
        }
        if dev.fd < 0 {
            dev.fd = Device::find_device(cfg.vid, cfg.pid);
            if dev.fd >= 0 {
                if dev.init_u(dev.fd, cfg.vid) {
                    ev_fd = EpollEvent { events: EPOLLIN, data: dev.fd as u64 };
                    unsafe { epoll_ctl(epfd, EPOLL_CTL_ADD, dev.fd, &mut ev_fd); }
                    have_fd = true;
                } else { dev.deinit(); }
            }
        }
        // flush due pending releases
        let now = std::time::Instant::now();
        let mut i = 0;
        while i < pending_releases.len() {
            if pending_releases[i].0 <= now {
                let emit = &pending_releases[i].1;
                let mut ev = InputEvent::default();
                unsafe {
                    ev.type_ = emit.ev_type; ev.code = emit.code; ev.value = 0;
                    write_ev(dev.ufd, &ev);
                    // write SYN_REPORT for the release
                    ev.type_ = EV_SYN as u16; ev.code = SYN_REPORT as u16; ev.value = 0;
                    write_ev(dev.ufd, &ev);
                }
                pending_releases.remove(i);
            } else { i += 1; }
        }

        let mut events = [EpollEvent::default(); 2];
        let timeout: i32 = if dev.fd < 0 { 1000 } else if pending_releases.is_empty() { -1 } else { 50 };
        if unsafe { epoll_wait(epfd, events.as_mut_ptr(), 2, timeout) } <= 0 { continue; }

        let mut fd_ready = false; let mut fd_hup = false;
        for ev in &events {
            if ev.data == ifd as u64 {
                let mut buf = [0u8; 1024];
                unsafe { while read(ifd, buf.as_mut_ptr(), buf.len()) > 0 {} }
            } else if have_fd && ev.data == dev.fd as u64 {
                fd_ready = true;
                if ev.events & (EPOLLHUP | EPOLLERR) != 0 { fd_hup = true; }
            }
        }
        if dev.fd < 0 || (!fd_ready && !fd_hup) { continue; }

        let mut shutdown = false;
        loop {
            let mut ev = InputEvent::default();
            let rb = unsafe { read(dev.fd, &mut ev as *mut _ as *mut u8, ev_size) };
            if rb != ev_size as isize {
                if (rb < 0 && get_errno() != EAGAIN) || fd_hup {
                    unsafe { epoll_ctl(epfd, EPOLL_CTL_DEL, dev.fd, &mut ev_fd); }
                    dev.deinit(); have_fd = false; shutdown = true;
                }
                break;
            }
            unsafe {
                match ev.type_ as i32 {
                    EV_ABS => match ev.code as u32 {
                        ABS_X  => { dev.lx = ev.value; dev.ld = true; }
                        ABS_Y  => { dev.ly = ev.value; dev.ld = true; }
                        ABS_RX => { dev.rx = ev.value; dev.rd = true; }
                        ABS_RY => { dev.ry = ev.value; dev.rd = true; }
                        ABS_Z  => {
                            let mut e = Event::Trigger { value: ev.value, side: Side::Left };
                            let (emits, dropped) = pipeline.run(&mut e, &cfg.values);
                            if !dropped && let Event::Trigger { value, .. } = e { let mut se = ev; se.value = value; write_ev(dev.ufd, &se); }
                            write_emits(dev.ufd, &ev, &emits, &mut pending_releases)
                        }
                        ABS_RZ => {
                            let mut e = Event::Trigger { value: ev.value, side: Side::Right };
                            let (emits, dropped) = pipeline.run(&mut e, &cfg.values);
                            if !dropped && let Event::Trigger { value, .. } = e { let mut se = ev; se.value = value; write_ev(dev.ufd, &se); }
                            write_emits(dev.ufd, &ev, &emits, &mut pending_releases)
                        }
                        _ => { write_ev(dev.ufd, &ev); }
                    },
                    EV_KEY => {
                        let mut e = Event::Button { code: ev.code, pressed: ev.value != 0 };
                        let (emits, dropped) = pipeline.run(&mut e, &cfg.values);
                        if !dropped && let Event::Button { pressed, .. } = e { let mut se = ev; se.value = if pressed { 1 } else { 0 }; write_ev(dev.ufd, &se); }
                        write_emits(dev.ufd, &ev, &emits, &mut pending_releases)
                    }
                    EV_SYN if ev.code as u32 == SYN_REPORT => {
                        let _ = fs::write(RAW_FILE_L, format!("{} {}", dev.lx, dev.ly));
                        let _ = fs::write(RAW_FILE_R, format!("{} {}", dev.rx, dev.ry));
                        if dev.ld {
                            let mut e = Event::Stick { x: dev.lx, y: dev.ly, side: Side::Left };
                            let (emits, _) = pipeline.run(&mut e, &cfg.values);
                            if let Event::Stick { x, y, .. } = e {
                                let mut se = ev; se.type_ = EV_ABS as u16;
                                se.code = ABS_X as u16; se.value = x; write_ev(dev.ufd, &se);
                                se.code = ABS_Y as u16; se.value = y; write_ev(dev.ufd, &se);
                            }
                            write_emits(dev.ufd, &ev, &emits, &mut pending_releases);
                            dev.ld = false;
                        }
                        if dev.rd {
                            let mut e = Event::Stick { x: dev.rx, y: dev.ry, side: Side::Right };
                            let (emits, _) = pipeline.run(&mut e, &cfg.values);
                            if let Event::Stick { x, y, .. } = e {
                                let mut se = ev; se.type_ = EV_ABS as u16;
                                se.code = ABS_RX as u16; se.value = x; write_ev(dev.ufd, &se);
                                se.code = ABS_RY as u16; se.value = y; write_ev(dev.ufd, &se);
                            }
                            write_emits(dev.ufd, &ev, &emits, &mut pending_releases);
                            dev.rd = false;
                        }
                        write_ev(dev.ufd, &ev);
                    }
                    _ => { write_ev(dev.ufd, &ev); }
                }
            }
        }
        if shutdown { continue; }
    }

fn write_emits(ufd: i32, tmpl: &InputEvent, emits: &[pipeline::EmitEvent], pending: &mut Vec<(std::time::Instant, pipeline::EmitEvent)>) {
    unsafe {
        for emit in emits {
            let mut se = *tmpl;
            se.type_ = emit.ev_type; se.code = emit.code; se.value = emit.value;
            write_ev(ufd, &se);
            if let Some(ms) = emit.hold_ms
                && emit.value == 1 && emit.ev_type == 1 {
                pending.push((std::time::Instant::now() + std::time::Duration::from_millis(ms),
                    pipeline::EmitEvent { ev_type: emit.ev_type, code: emit.code, value: 0, hold_ms: None }));
            }
        }
    }
}
}
