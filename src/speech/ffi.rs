//! Runtime FFI over libvosk.dll (pinned vosk-api v0.3.45) via libloading.
//!
//! Loaded at run time rather than linked so that a missing DLL or corrupt
//! model degrades to an error state (FR-9), never a crash or link failure.
//! Only the processing thread touches `Model`/`Recognizer`.
//!
//! The whole module is `dead_code` until Task 4's engine consumes it
//! (bin crates have no public API surface).

#![allow(dead_code)]

use libloading::Library;
use serde_json::Value;
use std::ffi::{c_char, c_int, c_void, CString};
use std::sync::Arc;

/// Opaque C types (vosk_api.h).
#[repr(C)]
pub struct VoskModel {
    _private: [u8; 0],
}
#[repr(C)]
pub struct VoskRecognizer {
    _private: [u8; 0],
}

#[derive(Debug)]
pub enum FfiError {
    DllLoad(String),
    Symbol(String),
    ModelCreateFailed,
    RecognizerCreateFailed,
}

impl std::fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FfiError::DllLoad(e) => write!(f, "loading libvosk.dll failed: {e}"),
            FfiError::Symbol(s) => write!(f, "missing symbol in libvosk.dll: {s}"),
            FfiError::ModelCreateFailed => write!(f, "vosk_model_new returned null"),
            FfiError::RecognizerCreateFailed => write!(f, "vosk_recognizer_new returned null"),
        }
    }
}

impl std::error::Error for FfiError {}

type ModelNewFn = unsafe extern "system" fn(*const c_char) -> *mut VoskModel;
type ModelFreeFn = unsafe extern "system" fn(*mut VoskModel);
type RecNewFn = unsafe extern "system" fn(*const VoskModel, f32) -> *mut VoskRecognizer;
type RecFreeFn = unsafe extern "system" fn(*mut VoskRecognizer);
type SetGrmFn = unsafe extern "system" fn(*mut VoskRecognizer, *const c_char);
type AcceptWaveFn = unsafe extern "system" fn(*mut VoskRecognizer, *const i16, c_int) -> c_int;
type ResultFn = unsafe extern "system" fn(*mut VoskRecognizer) -> *const c_char;
type LogLevelFn = unsafe extern "system" fn(c_int);

/// Owns libvosk.dll and its resolved function pointers.
pub struct VoskApi {
    #[allow(dead_code)] // kept alive so the fn pointers stay valid
    lib: Library,
    model_new: ModelNewFn,
    model_free: ModelFreeFn,
    rec_new: RecNewFn,
    rec_free: RecFreeFn,
    set_grm: SetGrmFn,
    accept_wave: AcceptWaveFn,
    partial_result: ResultFn,
    final_result: ResultFn,
    set_loglevel: LogLevelFn,
}

/// SAFETY: the Library and resolved fn pointers are process-global and safe to
/// share once loaded; the C API itself is only touched from one thread.
unsafe impl Send for VoskApi {}

impl VoskApi {
    /// Load libvosk.dll. `None` uses the default DLL search (repo root /
    /// exe directory); a path loads that exact file.
    ///
    /// # Safety
    /// Loads a native library and resolves C function pointers.
    pub unsafe fn load(path: Option<&str>) -> Result<Self, FfiError> {
        let lib = match path {
            Some(p) => Library::new(p),
            None => Library::new("libvosk.dll"),
        }
        .map_err(|e| FfiError::DllLoad(e.to_string()))?;
        let get = |name: &[u8]| -> Result<*mut c_void, FfiError> {
            lib.get::<*mut c_void>(name)
                .map(|s| *s)
                .map_err(|_| FfiError::Symbol(String::from_utf8_lossy(name).to_string()))
        };
        Ok(Self {
            model_new: unsafe {
                std::mem::transmute::<*mut c_void, ModelNewFn>(get(b"vosk_model_new\0")?)
            },
            model_free: unsafe {
                std::mem::transmute::<*mut c_void, ModelFreeFn>(get(b"vosk_model_free\0")?)
            },
            rec_new: unsafe {
                std::mem::transmute::<*mut c_void, RecNewFn>(get(b"vosk_recognizer_new\0")?)
            },
            rec_free: unsafe {
                std::mem::transmute::<*mut c_void, RecFreeFn>(get(b"vosk_recognizer_free\0")?)
            },
            set_grm: unsafe {
                std::mem::transmute::<*mut c_void, SetGrmFn>(get(b"vosk_recognizer_set_grm\0")?)
            },
            accept_wave: unsafe {
                std::mem::transmute::<*mut c_void, AcceptWaveFn>(get(
                    b"vosk_recognizer_accept_waveform_s\0",
                )?)
            },
            partial_result: unsafe {
                std::mem::transmute::<*mut c_void, ResultFn>(get(
                    b"vosk_recognizer_partial_result\0",
                )?)
            },
            final_result: unsafe {
                std::mem::transmute::<*mut c_void, ResultFn>(get(
                    b"vosk_recognizer_final_result\0",
                )?)
            },
            set_loglevel: unsafe {
                std::mem::transmute::<*mut c_void, LogLevelFn>(get(b"vosk_set_log_level\0")?)
            },
            lib,
        })
    }

    /// Silence the C library's logging (0 = off).
    pub fn set_loglevel(&self, level: i32) {
        unsafe { (self.set_loglevel)(level) }
    }
}

/// An open Vosk acoustic/language model. Drop frees it (`vosk_model_free`).
pub struct Model {
    api: Arc<VoskApi>,
    handle: *mut VoskModel,
}

/// SAFETY: only the processing thread touches the model.
unsafe impl Send for Model {}

impl Model {
    /// # Safety
    /// Calls into the loaded C library.
    pub unsafe fn new(api: &Arc<VoskApi>, path: &str) -> Result<Self, FfiError> {
        let cpath = CString::new(path).map_err(|_| FfiError::ModelCreateFailed)?;
        let handle = (api.model_new)(cpath.as_ptr());
        if handle.is_null() {
            return Err(FfiError::ModelCreateFailed);
        }
        Ok(Self {
            api: Arc::clone(api),
            handle,
        })
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        unsafe { (self.api.model_free)(self.handle) }
    }
}

/// A streaming recognizer. Drop frees it (`vosk_recognizer_free`), which
/// must happen before the model drop it borrows — it owns its `Model`.
pub struct Recognizer {
    model: Model,
    handle: *mut VoskRecognizer,
}

/// SAFETY: only the processing thread touches the recognizer.
unsafe impl Send for Recognizer {}

impl Recognizer {
    /// # Safety
    /// Calls into the loaded C library.
    pub unsafe fn new(model: Model, sample_rate: f32) -> Result<Self, FfiError> {
        let handle = (model.api.rec_new)(model.handle, sample_rate);
        if handle.is_null() {
            return Err(FfiError::RecognizerCreateFailed);
        }
        Ok(Self { model, handle })
    }

    /// Restrict recognition to a JSON array of phrases (grammar mode).
    pub fn set_grammar(&self, grammar_json: &str) {
        let c = CString::new(grammar_json.as_bytes().to_vec()).unwrap_or_default();
        unsafe { (self.model.api.set_grm)(self.handle, c.as_ptr()) }
    }

    /// Feed one chunk of 16 kHz mono samples. `true` = speech ended and a
    /// final result is available.
    pub fn accept_waveform(&self, data: &[i16]) -> bool {
        let out = unsafe {
            (self.model.api.accept_wave)(self.handle, data.as_ptr(), data.len() as c_int)
        };
        out != 0
    }

    /// `{"partial": "..."}` — the partial text, empty when none.
    pub fn partial_result(&self) -> String {
        self.result_text("partial")
    }

    /// `{"text": "..."}` — the final text, empty when none.
    pub fn final_result(&self) -> String {
        self.result_text("text")
    }

    fn result_text(&self, key: &str) -> String {
        let ptr = unsafe {
            if key == "text" {
                (self.model.api.final_result)(self.handle)
            } else {
                (self.model.api.partial_result)(self.handle)
            }
        };
        if ptr.is_null() {
            return String::new();
        }
        let raw = unsafe { std::ffi::CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        match serde_json::from_str::<Value>(&raw) {
            Ok(v) => v.get(key).and_then(Value::as_str).unwrap_or("").to_string(),
            Err(_) => String::new(),
        }
    }
}

impl Drop for Recognizer {
    fn drop(&mut self) {
        unsafe { (self.model.api.rec_free)(self.handle) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Run locally with: cargo test -- --ignored
    #[test]
    #[ignore = "requires local libvosk.dll and model/"]
    fn loads_dll_model_and_recognizes_silence() {
        let api = unsafe { VoskApi::load(None) }.expect("libvosk.dll next to exe");
        let api = Arc::new(api);
        let model = unsafe { Model::new(&api, "model") }.expect("model loads");
        let rec = unsafe { Recognizer::new(model, 16000.0) }.expect("recognizer");
        rec.set_grammar(
            crate::speech::grammar::build_grammar_json(&["hello world".to_string()]).as_str(),
        );
        let silence = vec![0i16; 1600];
        let _speech_ended = rec.accept_waveform(&silence);
        // Silence under a grammar restriction yields no words: the parsed
        // partial and final texts are empty strings (not raw JSON).
        let partial = rec.partial_result();
        assert!(partial.is_empty(), "got: {partial}");
        assert!(rec.final_result().is_empty(), "got: {}", rec.final_result());
    }
}
