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
    let mut config_path = "/sdcard/.keyforge/keyforge.conf".to_string();
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
    let _ = plugin::load_plugins(lua, &cfg.plugin_dir, &mut pipeline, &cfg.values);
    let mut dev = Device::new();
    let ev_size = std::mem::size_of::<InputEvent>();
    let mut pending_releases: Vec<(std::time::Instant, pipeline::EmitEvent)> = Vec::new();

    // inotify for device hotplug only
    let ifd = unsafe { inotify_init1(IN_NONBLOCK) };
    let mut have_inotify = false;
    if ifd >= 0 {
        if let Ok(cpath) = std::ffi::CString::new(INPUT_DIR) {
            unsafe { inotify_add_watch(ifd, cpath.as_ptr(), IN_CREATE | IN_DELETE); }
            have_inotify = true;
        }
    }

    let epfd = unsafe { epoll_create1(0) };
    if epfd < 0 { eprintln!("keyforge: epoll_create1 failed"); std::process::exit(1); }

    // Register inotify fd
    let mut ep_if = EpollEvent { events: EPOLLIN, data: 0 };
    if have_inotify {
        ep_if.data = ifd as u64;
        unsafe { epoll_ctl(epfd, EPOLL_CTL_ADD, ifd, &mut ep_if); }
    }

    let mut ep_dev = EpollEvent { events: EPOLLIN, data: 0 };
    let mut have_dev = false;

    // Find device and init uinput
    loop {
        dev.fd = Device::find_device(cfg.vid, cfg.pid);
        if dev.fd >= 0 {
            if dev.init_u(dev.fd, cfg.vid) { break; }
            dev.deinit();
        }
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
    ep_dev.data = dev.fd as u64;
    unsafe { epoll_ctl(epfd, EPOLL_CTL_ADD, dev.fd, &mut ep_dev); }
    have_dev = true;

    let mut last_cfg_check = std::time::Instant::now();
    loop {
        // Config reload check (every ~2s via timeout, or on inotify wake)
        let now = std::time::Instant::now();
        if now.duration_since(last_cfg_check).as_millis() >= 500 {
            last_cfg_check = now;
            let fresh = Config::load(config_path);
            let vid_changed = fresh.vid != cfg.vid || fresh.pid != cfg.pid;
            let settings_changed = fresh.values != cfg.values || fresh.plugin_dir != cfg.plugin_dir;
            if vid_changed || settings_changed {
                cfg = fresh;
                pipeline = Pipeline::new();
                let _ = plugin::load_plugins(lua, &cfg.plugin_dir, &mut pipeline, &cfg.values);
                if vid_changed && have_dev {
                    unsafe { epoll_ctl(epfd, EPOLL_CTL_DEL, dev.fd, &mut ep_dev); }
                    dev.deinit();
                    have_dev = false;
                }
            }
        }

        // Auto-connect if no device
        if !have_dev {
            loop {
                dev.fd = Device::find_device(cfg.vid, cfg.pid);
                if dev.fd >= 0 {
                    if dev.init_u(dev.fd, cfg.vid) { break; }
                    dev.deinit();
                }
                std::thread::sleep(std::time::Duration::from_millis(1000));
            }
            ep_dev.data = dev.fd as u64;
            unsafe { epoll_ctl(epfd, EPOLL_CTL_ADD, dev.fd, &mut ep_dev); }
            have_dev = true;
        }

        // Flush pending releases
        let mut ri = 0;
        while ri < pending_releases.len() {
            if pending_releases[ri].0 <= std::time::Instant::now() {
                let emit = &pending_releases[ri].1;
                let mut rev = InputEvent::default();
                rev.type_ = emit.ev_type; rev.code = emit.code; rev.value = 0;
                unsafe { write_ev(dev.ufd, &rev); }
                rev.type_ = EV_SYN as u16; rev.code = SYN_REPORT as u16; rev.value = 0;
                unsafe { write_ev(dev.ufd, &rev); }
                pending_releases.remove(ri);
            } else { ri += 1; }
        }

        // epoll_wait with timeout for periodic config checks
        let timeout: i32 = if pending_releases.is_empty() { 500 } else { 50 };
        let mut events = [EpollEvent::default(); 2];
        if unsafe { epoll_wait(epfd, events.as_mut_ptr(), 2, timeout) } <= 0 { continue; }

        let mut fd_ready = false; let mut fd_hup = false;
        for ev in &events {
            if have_inotify && ev.data == ifd as u64 {
                let mut buf = [0u8; 1024];
                unsafe { while read(ifd, buf.as_mut_ptr(), buf.len()) > 0 {} }
                last_cfg_check = std::time::Instant::now() - std::time::Duration::from_secs(10); // force config check
            } else if have_dev && ev.data == dev.fd as u64 {
                fd_ready = true;
                if ev.events & (EPOLLHUP | EPOLLERR) != 0 { fd_hup = true; }
            }
        }
        if !have_dev || (!fd_ready && !fd_hup) { continue; }

        // Read and process events
        let mut disconnected = false;
        loop {
            let mut iev = InputEvent::default();
            let rb = unsafe { read(dev.fd, &mut iev as *mut _ as *mut u8, ev_size) };
            if rb != ev_size as isize {
                if (rb < 0 && get_errno() != EAGAIN) || fd_hup { disconnected = true; }
                break;
            }
            unsafe {
                let mut skip = false;
                match iev.type_ as i32 {
                    EV_ABS => match iev.code as u32 {
                        ABS_X  => { dev.lx = iev.value; dev.ld = true; skip = true; }
                        ABS_Y  => { dev.ly = iev.value; dev.ld = true; skip = true; }
                        ABS_RX => { dev.rx = iev.value; dev.rd = true; skip = true; }
                        ABS_RY => { dev.ry = iev.value; dev.rd = true; skip = true; }
                        ABS_Z  => {
                            let mut e = Event::Trigger { value: iev.value, side: Side::Left };
                            let (emits, dropped) = pipeline.run(&mut e, &cfg.values);
                            if !dropped { iev.value = e.value(); }
                            for emit in &emits {
                                let mut se = iev; se.type_ = emit.ev_type; se.code = emit.code; se.value = emit.value;
                                write_ev(dev.ufd, &se);
                                if let Some(ms) = emit.hold_ms
                                    && emit.value == 1 && emit.ev_type == 1 {
                                    pending_releases.push((std::time::Instant::now() + std::time::Duration::from_millis(ms),
                                        pipeline::EmitEvent { ev_type: emit.ev_type, code: emit.code, value: 0, hold_ms: None }));
                                }
                            }
                        }
                        ABS_RZ => {
                            let mut e = Event::Trigger { value: iev.value, side: Side::Right };
                            let (emits, dropped) = pipeline.run(&mut e, &cfg.values);
                            if !dropped { iev.value = e.value(); }
                            for emit in &emits {
                                let mut se = iev; se.type_ = emit.ev_type; se.code = emit.code; se.value = emit.value;
                                write_ev(dev.ufd, &se);
                                if let Some(ms) = emit.hold_ms
                                    && emit.value == 1 && emit.ev_type == 1 {
                                    pending_releases.push((std::time::Instant::now() + std::time::Duration::from_millis(ms),
                                        pipeline::EmitEvent { ev_type: emit.ev_type, code: emit.code, value: 0, hold_ms: None }));
                                }
                            }
                        }
                        _ => {}
                    },
                    EV_KEY => {
                        let mut e = Event::Button { code: iev.code, pressed: iev.value != 0 };
                        let (emits, dropped) = pipeline.run(&mut e, &cfg.values);
                        if !dropped { iev.value = if e.pressed() { 1 } else { 0 }; }
                        for emit in &emits {
                            let mut se = iev; se.type_ = emit.ev_type; se.code = emit.code; se.value = emit.value;
                            write_ev(dev.ufd, &se);
                            if let Some(ms) = emit.hold_ms
                                && emit.value == 1 && emit.ev_type == 1 {
                                pending_releases.push((std::time::Instant::now() + std::time::Duration::from_millis(ms),
                                    pipeline::EmitEvent { ev_type: emit.ev_type, code: emit.code, value: 0, hold_ms: None }));
                            }
                        }
                    }
                    EV_SYN if iev.code as u32 == SYN_REPORT => {
                        let _ = fs::write(RAW_FILE_L, format!("{} {}", dev.lx, dev.ly));
                        let _ = fs::write(RAW_FILE_R, format!("{} {}", dev.rx, dev.ry));
                        if dev.ld {
                            let mut e = Event::Stick { x: dev.lx, y: dev.ly, side: Side::Left };
                            let (emits, _) = pipeline.run(&mut e, &cfg.values);
                            let mut se = iev; se.type_ = EV_ABS as u16;
                            se.code = ABS_X as u16; se.value = e.x(); write_ev(dev.ufd, &se);
                            se.code = ABS_Y as u16; se.value = e.y(); write_ev(dev.ufd, &se);
                            for emit in &emits {
                                let mut we = iev; we.type_ = emit.ev_type; we.code = emit.code; we.value = emit.value;
                                write_ev(dev.ufd, &we);
                                if let Some(ms) = emit.hold_ms
                                    && emit.value == 1 && emit.ev_type == 1 {
                                    pending_releases.push((std::time::Instant::now() + std::time::Duration::from_millis(ms),
                                        pipeline::EmitEvent { ev_type: emit.ev_type, code: emit.code, value: 0, hold_ms: None }));
                                }
                            }
                            dev.ld = false;
                        }
                        if dev.rd {
                            let mut e = Event::Stick { x: dev.rx, y: dev.ry, side: Side::Right };
                            let (emits, _) = pipeline.run(&mut e, &cfg.values);
                            let mut se = iev; se.type_ = EV_ABS as u16;
                            se.code = ABS_RX as u16; se.value = e.x(); write_ev(dev.ufd, &se);
                            se.code = ABS_RY as u16; se.value = e.y(); write_ev(dev.ufd, &se);
                            for emit in &emits {
                                let mut we = iev; we.type_ = emit.ev_type; we.code = emit.code; we.value = emit.value;
                                write_ev(dev.ufd, &we);
                                if let Some(ms) = emit.hold_ms
                                    && emit.value == 1 && emit.ev_type == 1 {
                                    pending_releases.push((std::time::Instant::now() + std::time::Duration::from_millis(ms),
                                        pipeline::EmitEvent { ev_type: emit.ev_type, code: emit.code, value: 0, hold_ms: None }));
                                }
                            }
                            dev.rd = false;
                        }
                    }
                    _ => {}
                }
                if !skip { write_ev(dev.ufd, &iev); }
            }
        }
        if disconnected {
            unsafe { epoll_ctl(epfd, EPOLL_CTL_DEL, dev.fd, &mut ep_dev); }
            dev.deinit();
            have_dev = false;
        }
    }
}
