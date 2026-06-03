use crate::pipeline::EmitEvent;
use mlua::{Lua, Table};
use std::sync::{Arc, Mutex};

/// Build the `pf` API table for a Lua plugin.
pub fn build_pf(
    lua: &Lua,
    emits_buf: &Arc<Mutex<Vec<EmitEvent>>>,
    drop_flag: &Arc<Mutex<bool>>,
    raw_x: i32,
    raw_y: i32,
) -> mlua::Result<Table> {
    let pf = lua.create_table()?;
    pf.set("version", "1.0.0")?;
    pf.set("raw_x", raw_x)?;
    pf.set("raw_y", raw_y)?;

    // event type constants
    pf.set("EV_KEY", 1)?;
    pf.set("EV_ABS", 3)?;

    // pf.emit(type, code, value [, hold_ms])
    let eb = emits_buf.clone();
    pf.set("emit", lua.create_function(move |_, (ev_type, code, value, hold_ms): (u16, u16, i32, Option<u64>)| {
        if let Ok(mut v) = eb.lock() { v.push(EmitEvent { ev_type, code, value, hold_ms }); }
        Ok(())
    })?)?;

    // pf.drop()
    let df = drop_flag.clone();
    pf.set("drop", lua.create_function(move |_, ()| {
        if let Ok(mut d) = df.lock() { *d = true; }
        Ok(())
    })?)?;

    // pf.log(msg)
    pf.set("log", lua.create_function(|_, msg: String| {
        eprintln!("keyforge[Lua]: {}", msg);
        Ok(())
    })?)?;

    Ok(pf)
}
