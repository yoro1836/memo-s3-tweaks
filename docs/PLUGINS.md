# KeyForge Plugin API

## Overview

KeyForge plugins are `.lua` files loaded from `/sdcard/.keyforge/plugins/`. Each plugin defines a `process(ev, cfg, pf)` function that receives every input event. Plugins run in a pipeline — output of one feeds into the next.

Virtual device exposes all KEY (0–767) and ABS (0–63) codes. Plugins can emit any event via `pf.emit()`.

## Plugin Structure

```lua
return {
  id = "my_plugin",
  name = "My Plugin",
  version = "1.0.0",
  author = "me",
  description = "What it does",
  settings = { ... },
  process = function(ev, cfg, pf)
    return ev
  end
}
```

## `ev` — Input Event

Every event has `.kind`:

| kind | Fields | Description |
|------|--------|-------------|
| `"stick"` | `x, y, side` | Analog stick. side: `"left"` / `"right"` |
| `"trigger"` | `value, side` | Trigger. value: 0–255 |
| `"button"` | `code, pressed` | Button. code: Linux key code |

Modify `ev` fields and return it. The pipeline writes the result to the virtual device.

## `pf` — Core API

| Symbol | Type | Description |
|--------|------|-------------|
| `pf.emit(type, code, value)` | function | Write raw evdev event |
| `pf.drop()` | function | Suppress current event |
| `pf.log(msg)` | function | Write to daemon log |
| `pf.version` | string | Daemon version |
| `pf.raw_x` / `pf.raw_y` | number | Original stick values |
| `pf.EV_KEY` | 1 | Event type constant |
| `pf.EV_ABS` | 3 | Event type constant |

## Setting Kinds

| kind | WebUI widget |
|------|-------------|
| `"permille"` | Slider (‰) + number input |
| `"toggle"` | Toggle switch |
| `"number"` | Number input |

Settings appear as `cfg.key` (always a string).

## Examples

### Remap a button

```lua
local A = 304; local B = 305
process = function(ev, cfg, pf)
  if ev.kind == "button" and ev.code == A then
    pf.emit(pf.EV_KEY, B, ev.pressed and 1 or 0)
    pf.drop()
  end
  return ev
end
```

### Stick deadzone

```lua
process = function(ev, cfg, pf)
  if ev.kind ~= "stick" then return ev end
  local dz = tonumber(cfg.threshold) or 0
  if dz <= 0 then return ev end
  local thr = dz * 32767 / 1000
  if ev.x * ev.x + ev.y * ev.y < thr * thr then
    ev.x = 0; ev.y = 0
  end
  return ev
end
```

### Trigger to button

```lua
local A = 304
process = function(ev, cfg, pf)
  if ev.kind == "trigger" and ev.side == "left" then
    if ev.value > 128 then
      pf.emit(pf.EV_KEY, A, 1)   -- press A when LT pulled past 50%
    else
      pf.emit(pf.EV_KEY, A, 0)   -- release A
    end
  end
  return ev
end
```

### Macro: combo on press

```lua
local A = 304; local B = 305
process = function(ev, cfg, pf)
  if ev.kind == "button" and ev.code == A and ev.pressed then
    pf.emit(pf.EV_KEY, B, 1)      -- press B
    -- B stays pressed until A is released
  elseif ev.kind == "button" and ev.code == A and not ev.pressed then
    pf.emit(pf.EV_KEY, B, 0)      -- release B
  end
  return ev
end
```

## Available Lua

Lua 5.4 standard library: `math`, `string`, `table`, `tonumber`, `tostring`, `pairs`, `ipairs`.
