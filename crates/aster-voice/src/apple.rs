//! Apple's on-device speech recognizer: words while the speaker talks, with no
//! key and no network. macOS asks the app responsible for a process for speech
//! access, so [`relaunch_as_own_app`] makes `aster` that app.

#![allow(unsafe_code)]

use std::ffi::{CString, OsString, c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, Bool};
use objc2_avf_audio::{
    AVAudioApplication, AVAudioApplicationRecordPermission, AVAudioCommonFormat, AVAudioFormat,
    AVAudioPCMBuffer,
};
use objc2_foundation::{NSError, NSOperationQueue};
use objc2_speech::{
    SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionResult, SFSpeechRecognizer,
    SFSpeechRecognizerAuthorizationStatus,
};

use crate::{Heard, VoiceError};

const ASK_TIMEOUT: Duration = Duration::from_secs(60);
const FINAL_TIMEOUT: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(20);

unsafe extern "C" {
    fn responsibility_get_pid_responsible_for_pid(pid: libc::pid_t) -> libc::pid_t;
    fn responsibility_spawnattrs_setdisclaim(
        attrs: *mut libc::posix_spawnattr_t,
        disclaim: c_int,
    ) -> c_int;
    static environ: *const *mut c_char;
}

/// Runs this same command again as its own app, so permission prompts name
/// `aster` and read its Info.plist instead of the terminal's or editor's. The
/// child shares stdin and stdout. `None` means this process already is its own
/// app and should carry on; otherwise the child's exit code.
pub fn relaunch_as_own_app() -> std::io::Result<Option<i32>> {
    let pid = std::process::id() as libc::pid_t;
    if unsafe { responsibility_get_pid_responsible_for_pid(pid) } == pid {
        return Ok(None);
    }
    let exe = std::env::current_exe()?;
    let cstr = |s: OsString| CString::new(s.as_bytes()).map_err(std::io::Error::other);
    let path = cstr(exe.into_os_string())?;
    let args = std::env::args_os()
        .map(cstr)
        .collect::<Result<Vec<_>, _>>()?;
    let mut argv: Vec<*mut c_char> = args.iter().map(|a| a.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());

    let mut attrs = std::mem::MaybeUninit::<libc::posix_spawnattr_t>::uninit();
    let mut child: libc::pid_t = 0;
    let rc = unsafe {
        libc::posix_spawnattr_init(attrs.as_mut_ptr());
        responsibility_spawnattrs_setdisclaim(attrs.as_mut_ptr(), 1);
        let rc = libc::posix_spawn(
            &mut child,
            path.as_ptr(),
            std::ptr::null(),
            attrs.as_ptr(),
            argv.as_ptr(),
            environ,
        );
        libc::posix_spawnattr_destroy(attrs.as_mut_ptr());
        rc
    };
    if rc != 0 {
        return Err(std::io::Error::from_raw_os_error(rc));
    }
    let mut status: c_int = 0;
    if unsafe { libc::waitpid(child, &mut status, 0) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Some(if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        1
    }))
}

/// Asks for the microphone before recording, so the first words are not lost
/// to silence while the prompt is up. Systems before macOS 14 ask on first use.
pub fn ask_for_microphone() -> Result<(), VoiceError> {
    if AnyClass::get(c"AVAudioApplication").is_none() {
        return Ok(());
    }
    let app = unsafe { AVAudioApplication::sharedInstance() };
    let granted = match unsafe { app.recordPermission() } {
        AVAudioApplicationRecordPermission::Granted => true,
        AVAudioApplicationRecordPermission::Undetermined => {
            let (tx, rx) = mpsc::channel();
            let block = RcBlock::new(move |granted: Bool| {
                let _ = tx.send(granted.as_bool());
            });
            unsafe { AVAudioApplication::requestRecordPermissionWithCompletionHandler(&block) };
            rx.recv_timeout(ASK_TIMEOUT).unwrap_or(false)
        }
        _ => false,
    };
    if granted {
        Ok(())
    } else {
        Err(VoiceError::NoMicrophone("microphone access refused".into()))
    }
}

/// Recognizes `audio` (mono slices at `sample_rate`) on this machine until the
/// sender hangs up, then sends the settled text. Fails when speech access is
/// refused or the user's language has no on-device model.
pub fn listen_on_device(
    audio: mpsc::Receiver<Vec<i16>>,
    sample_rate: u32,
) -> Result<mpsc::Receiver<Heard>, VoiceError> {
    authorize()?;
    let (ready, started) = mpsc::channel();
    let (heard, words) = mpsc::channel();
    std::thread::spawn(move || recognize(&audio, sample_rate, &ready, heard));
    match started.recv() {
        Ok(result) => result.map(|()| words),
        Err(_) => Err(VoiceError::Unsupported),
    }
}

fn authorize() -> Result<(), VoiceError> {
    let status = unsafe { SFSpeechRecognizer::authorizationStatus() };
    let status = if status == SFSpeechRecognizerAuthorizationStatus::NotDetermined {
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |status: SFSpeechRecognizerAuthorizationStatus| {
            let _ = tx.send(status);
        });
        unsafe { SFSpeechRecognizer::requestAuthorization(&block) };
        rx.recv_timeout(ASK_TIMEOUT).unwrap_or(status)
    } else {
        status
    };
    if status == SFSpeechRecognizerAuthorizationStatus::Authorized {
        Ok(())
    } else {
        Err(VoiceError::SpeechNotAllowed)
    }
}

fn recognize(
    audio: &mpsc::Receiver<Vec<i16>>,
    sample_rate: u32,
    ready: &mpsc::Sender<Result<(), VoiceError>>,
    heard: mpsc::Sender<Heard>,
) {
    let recognizer = unsafe { SFSpeechRecognizer::init(SFSpeechRecognizer::alloc()) };
    let format = unsafe {
        AVAudioFormat::initWithCommonFormat_sampleRate_channels_interleaved(
            AVAudioFormat::alloc(),
            AVAudioCommonFormat::PCMFormatFloat32,
            f64::from(sample_rate),
            1,
            false,
        )
    };
    let usable =
        |r: &SFSpeechRecognizer| unsafe { r.isAvailable() && r.supportsOnDeviceRecognition() };
    let (Some(recognizer), Some(format)) = (recognizer.filter(|r| usable(r)), format) else {
        let _ = ready.send(Err(VoiceError::Unsupported));
        return;
    };

    let queue = NSOperationQueue::new();
    unsafe {
        queue.setMaxConcurrentOperationCount(1);
        recognizer.setQueue(&queue);
    }
    let request = unsafe { SFSpeechAudioBufferRecognitionRequest::new() };
    unsafe {
        request.setShouldReportPartialResults(true);
        request.setRequiresOnDeviceRecognition(true);
        request.setAddsPunctuation(true);
    }
    let done = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&done);
    let handler = RcBlock::new(
        move |result: *mut SFSpeechRecognitionResult, error: *mut NSError| {
            if let Some(result) = unsafe { result.as_ref() } {
                let text = unsafe { result.bestTranscription().formattedString() }.to_string();
                if unsafe { result.isFinal() } {
                    let _ = heard.send(Heard::Final(text));
                    finished.store(true, Ordering::Relaxed);
                } else {
                    let _ = heard.send(Heard::Partial(text));
                }
            } else if !error.is_null() {
                finished.store(true, Ordering::Relaxed);
            }
        },
    );
    let task = unsafe { recognizer.recognitionTaskWithRequest_resultHandler(&request, &handler) };
    let _ = ready.send(Ok(()));

    while let Ok(samples) = audio.recv() {
        if let Some(buffer) = pcm_buffer(&format, &samples) {
            unsafe { request.appendAudioPCMBuffer(&buffer) };
        }
    }
    unsafe { request.endAudio() };
    let deadline = Instant::now() + FINAL_TIMEOUT;
    while !done.load(Ordering::Relaxed) && Instant::now() < deadline {
        std::thread::sleep(POLL);
    }
    unsafe { task.cancel() };
}

fn pcm_buffer(format: &AVAudioFormat, samples: &[i16]) -> Option<Retained<AVAudioPCMBuffer>> {
    let frames = u32::try_from(samples.len()).ok()?;
    let buffer = unsafe {
        AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(AVAudioPCMBuffer::alloc(), format, frames)
    }?;
    unsafe {
        buffer.setFrameLength(frames);
        let channel: NonNull<f32> = *buffer.floatChannelData();
        let out = std::slice::from_raw_parts_mut(channel.as_ptr(), samples.len());
        for (dst, &src) in out.iter_mut().zip(samples) {
            *dst = f32::from(src) / f32::from(i16::MAX);
        }
    }
    Some(buffer)
}
