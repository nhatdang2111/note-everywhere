//! Cryptographically secure random bytes from the Workers runtime
//! (`crypto.getRandomValues`).

use js_sys::{Function, Reflect, Uint8Array};
use worker::wasm_bindgen::{JsCast, JsValue};
use worker::{Error, Result, js_sys};

fn fill(buf: &mut [u8]) -> Result<()> {
    let global = js_sys::global();
    let crypto = Reflect::get(&global, &JsValue::from_str("crypto"))?;
    let get_random_values: Function = Reflect::get(&crypto, &JsValue::from_str("getRandomValues"))?
        .dyn_into()
        .map_err(|_| Error::RustError("crypto.getRandomValues is missing".into()))?;
    // The Web Crypto API refuses more than 65536 bytes per call.
    for chunk in buf.chunks_mut(65_536) {
        let array = Uint8Array::new_with_length(chunk.len() as u32);
        get_random_values.call1(&crypto, &array)?;
        array.copy_to(chunk);
    }
    Ok(())
}

pub fn bytes<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0u8; N];
    fill(&mut out)?;
    Ok(out)
}
