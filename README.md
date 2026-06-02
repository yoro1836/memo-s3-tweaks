# KeyForge

Lua-scriptable input automation for Android. Intercept, transform, and emit
evdev events at the kernel level — any key, any axis, any device.

Like AutoHotkey, but runs through Shizuku with direct access to `/dev/input`.

## How It Works

1. **KeyForge creates a virtual input device** via uinput — all keys and axes exposed
2. **Your physical device is grabbed exclusively** — raw input goes through the pipeline
3. **Lua plugins process every event** — modify, drop, or emit new ones
4. **Transformed output goes to the virtual device** — any app sees it as real input

## Features

- **Lua pipeline** — chain plugins that see every stick, trigger, and button event
- **Low-level API** — `pf.emit(type, code, value)` and `pf.drop()` for full control
- **All 768 keys + 64 axes** — emit keyboard keys, mouse buttons, gamepad inputs, anything
- **Native Android UI** — Compose + Material 3, Monet dynamic color, light/dark mode
- **Device scanner** — auto-detect connected controllers via `getevent`
- **Plugin manager** — install, remove, enable/disable `.lua` files
- **Hot reload** — config changes apply without restart
- **Shizuku-powered** — no root, just grant Shizuku permission once

## Install

1. Install [Shizuku](https://shizuku.rikka.app/)
2. Download `keyforge.apk` from [Releases](https://github.com/yoro1836/keyforge/releases)
3. Install APK, open app, grant Shizuku permission
4. Tap ▶ to start the daemon
5. Add plugins from [keyforge-plugins](https://github.com/yoro1836/keyforge-plugins)

## Plugin API

```lua
-- remap A to B
local A, B = 304, 305
return {
  id = "swap_ab", name = "Swap A/B", version = "1.0.0", author = "me",
  process = function(ev, cfg, pf)
    if ev.kind == "button" and ev.code == A then
      pf.emit(pf.EV_KEY, B, ev.pressed and 1 or 0)
      pf.drop()
    end
    return ev
  end
}
```

See [docs/PLUGINS.md](docs/PLUGINS.md) for the full API reference.

## Structure

```
app/           Android app (Compose UI, Shizuku UserService)
daemon/        Rust daemon (evdev → pipeline → uinput)
docs/          Plugin API reference
```

## Build

```sh
# daemon
cd daemon && cargo build --release --target aarch64-linux-android

# apk (debug)
./gradlew assembleDebug

# apk (release, needs keystore)
./gradlew assembleRelease -PKEYSTORE_FILE=keyforge.jks
```

CI builds debug + release APKs on `workflow_dispatch`.

## License

MIT — see [LICENSE](LICENSE)
