#![deny(unsafe_op_in_unsafe_fn)]
#![allow(non_snake_case)]

mod logging {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn timestamp() -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0));

        let seconds = now.as_secs();
        let millis = now.subsec_millis();

        format!("[{}.{:03}]", seconds, millis)
    }

    #[macro_export]
    macro_rules! log_ts {
        ($($arg:tt)*) => {
            if cfg!(debug_assertions) {
                println!("{} {}", $crate::logging::timestamp(), format!($($arg)*));
            }
        };
    }
}

use std::cell::OnceCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSPasteboard, NSTextField, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    ns_string, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

// Simple container to hold application state
struct AppState {
    window: OnceCell<Retained<NSWindow>>,
    text_field: OnceCell<Retained<NSTextField>>,
    change_count: AtomicU64,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            window: OnceCell::new(),
            text_field: OnceCell::new(),
            change_count: AtomicU64::new(0),
        }
    }
}

// Create the text field
fn create_text_field(window: &NSWindow, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let content_view = window.contentView().unwrap();
    let content_frame = content_view.bounds();

    // Create text field with inset from window edges
    let padding = 20.0;
    let text_field_frame = NSRect::new(
        NSPoint::new(padding, padding),
        NSSize::new(
            content_frame.size.width - (padding * 2.0),
            content_frame.size.height - (padding * 2.0),
        ),
    );

    let text_field =
        unsafe { NSTextField::initWithFrame(NSTextField::alloc(mtm), text_field_frame) };

    // Configure text field properties
    unsafe {
        text_field.setEditable(false);
        text_field.setBezeled(false);
        text_field.setDrawsBackground(false);
        text_field.setSelectable(true);
        text_field.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );

        // Set an initial value
        text_field.setStringValue(ns_string!("Monitoring clipboard... Copy something!"));

        // Add to content view
        content_view.addSubview(&text_field);
    }

    text_field
}

// Create the window
fn create_window(mtm: MainThreadMarker) -> Retained<NSWindow> {
    let window_frame = NSRect::new(NSPoint::new(100., 100.), NSSize::new(600., 400.));
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Resizable
        | NSWindowStyleMask::Miniaturizable;

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            window_frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };

    unsafe {
        window.setReleasedWhenClosed(false);
    }

    window
}

// Updates the text field with clipboard content safely
fn update_text_field_with_clipboard_content(
    text_field: &NSTextField,
    clipboard_text: Option<&NSString>,
) {
    if let Some(clipboard_text) = clipboard_text {
        let text_length = clipboard_text.length();

        // Safety check to prevent hanging with terminal content
        let try_using_clipboard = if text_length > 0 {
            let raw_chars = clipboard_text.UTF8String();

            if !raw_chars.is_null() {
                let bytes_to_examine = std::cmp::min(text_length, 20) as usize;
                let slice =
                    unsafe { std::slice::from_raw_parts(raw_chars as *const u8, bytes_to_examine) };

                // Check for control characters or ANSI escape codes
                let has_control_chars = slice
                    .iter()
                    .any(|&b| b < 32 && b != b'\t' && b != b'\n' && b != b'\r');
                let has_escape_sequence = slice.windows(2).any(|w| w == [0x1b, b'[']);

                !(has_control_chars || has_escape_sequence)
            } else {
                false
            }
        } else {
            true
        };

        if try_using_clipboard {
            unsafe { text_field.setStringValue(clipboard_text) };
        } else {
            // Display a placeholder message for terminal content
            unsafe {
                text_field.setStringValue(ns_string!(
                    "[Terminal text - copied but not displayed for stability]"
                ));
            }
        }
    } else {
        // No text content available
        unsafe {
            text_field.setStringValue(ns_string!("Clipboard contains non-text content"));
        }
    }
}

/// Update the UI with the clipboard content
fn update_from_clipboard(text_field: &NSTextField) {
    // Get the pasteboard and try to get text from it
    let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
    let string_type = ns_string!("public.utf8-plain-text");
    let clipboard_data = unsafe { pasteboard.stringForType(string_type) };

    // Update the text field
    update_text_field_with_clipboard_content(text_field, clipboard_data.as_ref());
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "AppDelegate"]
    #[ivars = AppState]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn applicationDidFinishLaunching(&self, _notification: &NSNotification) {
            log_ts!("Application startup - beginning initialization");
            let mtm = self.mtm();

            // Create and setup the window
            let window = create_window(mtm);
            let _ = self.ivars().window.set(window.clone());
            window.setTitle(ns_string!("Clipboard Viewer"));
            window.center();

            // Set delegate to handle window close
            unsafe {
                window.setDelegate(Some(ProtocolObject::from_ref(self)));
            }

            // Create and setup the text field
            let text_field = create_text_field(&window, mtm);
            let _ = self.ivars().text_field.get_or_init(|| text_field.clone());

            // Get and store initial clipboard change count
            let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
            let initial_count = unsafe { pasteboard.changeCount() as u64 };
            self.ivars()
                .change_count
                .store(initial_count, Ordering::SeqCst);

            // Display initial clipboard contents
            if let Some(text_field) = self.ivars().text_field.get() {
                update_from_clipboard(text_field);
            }

            // Set up timer to check for clipboard changes
            let this = self as *const _ as usize;
            let timer_block = RcBlock::new(move |_timer: *mut objc2_foundation::NSTimer| {
                let this = unsafe { &*(this as *const AppDelegate) };

                // Check if clipboard has changed
                let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
                let current_count = unsafe { pasteboard.changeCount() as u64 };
                let stored_count = this.ivars().change_count.load(Ordering::SeqCst);

                if current_count != stored_count {
                    // Update stored count
                    this.ivars()
                        .change_count
                        .store(current_count, Ordering::SeqCst);

                    // Update UI with new clipboard content
                    if let Some(text_field) = this.ivars().text_field.get() {
                        update_from_clipboard(text_field);
                    }
                }
            });

            // Create a timer to check clipboard every 0.5 seconds
            unsafe {
                let _ = objc2_foundation::NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    0.5,
                    ProtocolObject::from_ref(self),
                    objc::sel!(timerFired:),
                    None,
                    true,
                );

                // Store the block for the timer
                let _ = objc2_foundation::objc_setAssociatedObject(
                    self as *const _ as *mut objc2::runtime::Object,
                    b"timerBlock\0".as_ptr() as *const i8,
                    &*timer_block as *const _ as *mut objc2::runtime::Object,
                    objc2_foundation::objc_AssociationPolicy::OBJC_ASSOCIATION_RETAIN,
                );
            }

            // Activate app and make window visible
            let app = NSApplication::sharedApplication(mtm);
            unsafe {
                app.activate();
            }
            window.makeKeyAndOrderFront(None);

            log_ts!("Initialization complete - application ready");
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowWillClose:))]
        fn windowWillClose(&self, _notification: &NSNotification) {
            log_ts!("Window is closing - stopping application");

            // Terminate the application
            let mtm = self.mtm();
            let app = NSApplication::sharedApplication(mtm);
            unsafe { app.terminate(None) };
        }
    }
);

impl AppDelegate {
    #[allow(non_snake_case)]
    unsafe extern "C" fn timerFired(&self, _timer: *mut objc2_foundation::NSTimer) {
        // This will be called by the timer, but we delegate to the block
        let block_ptr = objc2_foundation::objc_getAssociatedObject(
            self as *const _ as *mut objc2::runtime::Object,
            b"timerBlock\0".as_ptr() as *const i8,
        );

        if !block_ptr.is_null() {
            let block = block_ptr as *const RcBlock<(*mut objc2_foundation::NSTimer,), ()>;
            (*block).call((_timer,));
        }
    }

    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        log_ts!("Creating new AppDelegate instance");
        let this = Self::alloc(mtm).set_ivars(AppState::default());
        let result = unsafe { msg_send![super(this), init] };
        log_ts!("AppDelegate instance created");
        result
    }
}

fn main() {
    log_ts!("Application starting");

    // Initialize on the main thread
    let mtm = MainThreadMarker::new().expect("Not running on main thread");

    // Get the shared application instance
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    // Create our app delegate
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    log_ts!("Starting application run loop");
    app.run();
}
