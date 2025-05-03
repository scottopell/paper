#![deny(unsafe_op_in_unsafe_fn)]
#![allow(non_snake_case)]

use std::cell::OnceCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSPasteboard, NSTextField, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    ns_string, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
};

// Function to get current timestamp in milliseconds for logging
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0));

    let seconds = now.as_secs();
    let millis = now.subsec_millis();

    // Format: seconds.milliseconds
    format!("[{}.{:03}]", seconds, millis)
}

// Macro for timestamped logging
macro_rules! log_ts {
    ($($arg:tt)*) => {
        println!("{} {}", timestamp(), format!($($arg)*));
    };
}

#[derive(Debug, Default)]
struct AppDelegateIvars {
    window: OnceCell<Retained<NSWindow>>,
    text_field: OnceCell<Retained<NSTextField>>,
    change_count: AtomicU64,
    should_check_clipboard: AtomicU64, // Used as a boolean value to control the loop
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "AppDelegate"]
    #[ivars = AppDelegateIvars]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn applicationDidFinishLaunching(&self, _notification: &NSNotification) {
            log_ts!("Application startup - beginning initialization");

            let mtm = self.mtm();
            log_ts!("Created MainThreadMarker");

            // Create and setup the window
            log_ts!("Creating window...");
            let window = self.create_window(mtm);
            let _ = self.ivars().window.set(window.clone());
            log_ts!("Window created and stored");

            window.setTitle(ns_string!("Clipboard Viewer"));
            window.center();
            log_ts!("Window configured");

            // Create and setup the text field
            log_ts!("Setting up text field...");
            self.setup_text_field(&window, mtm);
            log_ts!("Text field setup complete");

            // Get the general pasteboard and its current change count
            log_ts!("Accessing clipboard...");
            let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
            log_ts!("NSPasteboard::generalPasteboard() completed");

            log_ts!("Getting clipboard change count...");
            let initial_change_count = unsafe { pasteboard.changeCount() };
            log_ts!("pasteboard.changeCount() returned {}", initial_change_count);

            log_ts!("Storing initial change count: {}", initial_change_count);
            self.ivars()
                .change_count
                .store(initial_change_count as u64, Ordering::SeqCst);

            // Display initial clipboard contents
            log_ts!("Updating text field with initial clipboard contents...");
            self.update_text_from_clipboard();
            log_ts!("Initial clipboard display complete");

            // Set flag to start checking clipboard
            log_ts!("Enabling clipboard monitoring");
            self.ivars()
                .should_check_clipboard
                .store(1, Ordering::SeqCst);

            // Start clipboard check loop
            log_ts!("Starting clipboard check loop...");
            self.start_clipboard_check_loop();
            log_ts!("Clipboard monitoring thread started");

            // Activate app and make window visible
            log_ts!("Activating application...");
            let app = NSApplication::sharedApplication(mtm);

            log_ts!("About to activate app");
            unsafe {
                app.activate();
            }
            log_ts!("App activated");

            log_ts!("Making window visible...");
            window.makeKeyAndOrderFront(None);
            log_ts!("Initialization complete - application ready");
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowWillClose:))]
        fn windowWillClose(&self, _notification: &NSNotification) {
            log_ts!("Window is closing - stopping clipboard monitor");
            // Stop the clipboard check loop
            self.ivars()
                .should_check_clipboard
                .store(0, Ordering::SeqCst);

            let mtm = self.mtm();
            let app = NSApplication::sharedApplication(mtm);
            log_ts!("Terminating application");
            unsafe { app.terminate(None) };
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        log_ts!("Creating new AppDelegate instance");
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars::default());
        let result = unsafe { msg_send![super(this), init] };
        log_ts!("AppDelegate instance created");
        result
    }

    fn create_window(&self, mtm: MainThreadMarker) -> Retained<NSWindow> {
        log_ts!("Setting up window frame and style");
        let window_frame = NSRect::new(NSPoint::new(100., 100.), NSSize::new(600., 400.));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Resizable
            | NSWindowStyleMask::Miniaturizable;

        log_ts!("Allocating and initializing NSWindow");
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                window_frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };

        // Set delegate to handle window close
        log_ts!("Setting window delegate");
        unsafe {
            window.setDelegate(Some(ProtocolObject::from_ref(self)));
            window.setReleasedWhenClosed(false);
        }

        log_ts!("Window creation complete");
        window
    }

    fn setup_text_field(&self, window: &NSWindow, mtm: MainThreadMarker) {
        log_ts!("Getting content view from window");
        let content_view = window.contentView().unwrap();
        let content_frame = content_view.bounds();
        log_ts!("Content frame: {:?}", content_frame);

        // Create text field with inset from window edges
        let padding = 20.0;
        let text_field_frame = NSRect::new(
            NSPoint::new(padding, padding),
            NSSize::new(
                content_frame.size.width - (padding * 2.0),
                content_frame.size.height - (padding * 2.0),
            ),
        );
        log_ts!("Text field frame: {:?}", text_field_frame);

        log_ts!("Creating NSTextField");
        let text_field =
            unsafe { NSTextField::initWithFrame(NSTextField::alloc(mtm), text_field_frame) };

        log_ts!("Configuring text field properties");

        // Configure text field properties
        unsafe {
            text_field.setEditable(false);
            text_field.setBezeled(false);
            text_field.setDrawsBackground(false);
            text_field.setSelectable(true); // Allow selecting text for copying
            text_field.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );

            // Set an initial value
            log_ts!("Setting initial text field value");
            text_field.setStringValue(ns_string!("Monitoring clipboard... Copy something!"));

            // Add to content view
            log_ts!("Adding text field to content view");
            content_view.addSubview(&text_field);
        }

        // Store the text field
        log_ts!("Storing text field in AppDelegate");
        let _ = self.ivars().text_field.set(text_field);
        log_ts!("Text field setup complete");
    }

    // Update text field with current clipboard contents
    fn update_text_from_clipboard(&self) {
        log_ts!("update_text_from_clipboard: Starting clipboard update");

        // First, collect all clipboard data on the current thread
        log_ts!("update_text_from_clipboard: Getting pasteboard");
        let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
        log_ts!("update_text_from_clipboard: NSPasteboard::generalPasteboard() completed");

        // Try to get text from the pasteboard
        log_ts!("update_text_from_clipboard: Creating string type for pasteboard query");
        let string_type = ns_string!("public.utf8-plain-text");

        log_ts!("update_text_from_clipboard: About to read clipboard content");
        let start_time = std::time::Instant::now();
        let clipboard_data = unsafe { pasteboard.stringForType(string_type) };
        let elapsed = start_time.elapsed();
        log_ts!(
            "update_text_from_clipboard: stringForType completed in {:?}",
            elapsed
        );

        // Now dispatch the UI update to the main thread
        let app_delegate_ptr = self as *const _ as usize;

        // Spawn work on the main thread
        log_ts!("update_text_from_clipboard: Dispatching UI update to main thread");

        // Create a block for the main thread operation
        let block = RcBlock::new({
            // Clone values that need to be moved
            let clipboard_data = clipboard_data.clone();

            move || {
                log_ts!("update_text_from_clipboard [main thread]: Starting UI update");

                // Access app delegate on main thread
                let app_delegate = unsafe { &*(app_delegate_ptr as *const AppDelegate) };

                if let Some(text_field) = app_delegate.ivars().text_field.get() {
                    if let Some(clipboard_text) = clipboard_data.as_ref() {
                        let text_length = clipboard_text.length();
                        log_ts!("update_text_from_clipboard [main thread]: Found text in clipboard, length: {}", text_length);

                        // Safety check to prevent hanging with terminal content
                        let try_using_clipboard = {
                            // Simple heuristic: Examine up to 20 bytes of the content to detect terminal content
                            if text_length > 0 {
                                // We'll check the first few bytes to see if they contain control characters
                                let raw_chars = clipboard_text.UTF8String();

                                if !raw_chars.is_null() {
                                    let bytes_to_examine = std::cmp::min(text_length, 20) as usize;
                                    let slice = unsafe {
                                        std::slice::from_raw_parts(
                                            raw_chars as *const u8,
                                            bytes_to_examine,
                                        )
                                    };

                                    // Log the bytes for debugging
                                    let hex_repr: Vec<String> =
                                        slice.iter().map(|b| format!("{:02x}", b)).collect();
                                    log_ts!("update_text_from_clipboard [main thread]: First bytes in hex: {:?}", hex_repr);

                                    // Check for control characters or ANSI escape codes often found in terminal output
                                    let has_control_chars = slice
                                        .iter()
                                        .any(|&b| b < 32 && b != b'\t' && b != b'\n' && b != b'\r');
                                    let has_escape_sequence =
                                        slice.windows(2).any(|w| w == [0x1b, b'[']);

                                    if has_control_chars || has_escape_sequence {
                                        log_ts!("update_text_from_clipboard [main thread]: Terminal control characters detected - using safe mode");
                                        false
                                    } else {
                                        log_ts!("update_text_from_clipboard [main thread]: No control characters detected - using regular mode");
                                        true
                                    }
                                } else {
                                    log_ts!("update_text_from_clipboard [main thread]: UTF8String() returned null - using safe mode");
                                    false
                                }
                            } else {
                                log_ts!("update_text_from_clipboard [main thread]: Empty text - using regular mode");
                                true
                            }
                        };

                        // For safety, wrap the update in a timeout
                        log_ts!("update_text_from_clipboard [main thread]: Updating text field with clipboard content");

                        if try_using_clipboard {
                            // Try to show the content with a timer to measure performance
                            let start_set_time = std::time::Instant::now();
                            log_ts!(
                                "update_text_from_clipboard [main thread]: setStringValue starting"
                            );
                            unsafe { text_field.setStringValue(&clipboard_text) };
                            let set_elapsed = start_set_time.elapsed();
                            log_ts!("update_text_from_clipboard [main thread]: setStringValue completed in {:?}", set_elapsed);
                        } else {
                            // Display a placeholder message instead
                            unsafe {
                                text_field.setStringValue(ns_string!(
                                    "[Terminal text - copied but not displayed for stability]"
                                ));
                            }
                            log_ts!("update_text_from_clipboard [main thread]: Used safe display mode for terminal content");
                        }

                        log_ts!("update_text_from_clipboard [main thread]: Text field updated with clipboard content");
                    } else {
                        log_ts!(
                            "update_text_from_clipboard [main thread]: No text content in clipboard"
                        );
                        // No text content available
                        log_ts!("update_text_from_clipboard [main thread]: Setting 'non-text content' message");
                        unsafe {
                            text_field
                                .setStringValue(ns_string!("Clipboard contains non-text content"));
                        }
                        log_ts!("update_text_from_clipboard [main thread]: Message set");
                    }
                } else {
                    log_ts!("update_text_from_clipboard [main thread]: Text field not found!");
                }
                log_ts!("update_text_from_clipboard [main thread]: UI update complete");
            }
        });

        // Get the main queue and schedule our block
        let queue = unsafe { objc2_foundation::NSOperationQueue::mainQueue() };
        unsafe {
            queue.addOperationWithBlock(&block);
        }

        log_ts!("update_text_from_clipboard: Dispatched UI update to main thread");
    }

    fn start_clipboard_check_loop(&self) {
        log_ts!("Starting clipboard monitoring setup");

        // Create a queue for the clipboard monitoring
        let clipboard_queue = DispatchQueue::new("com.scottopell.paperclip.monitor", None);

        // Store pointers to the data we need to access
        let should_check_ptr = &self.ivars().should_check_clipboard as *const _ as usize;
        let change_count_ptr = &self.ivars().change_count as *const _ as usize;
        let app_delegate_ptr = self as *const _ as usize;

        // Launch the monitoring loop
        unsafe {
            clipboard_queue.exec_async(move || {
                // Create a loop that keeps checking the clipboard
                loop {
                    // Get the should_check flag
                    let should_check = &*(should_check_ptr as *const AtomicU64);

                    // If we should stop checking, exit the loop
                    if should_check.load(Ordering::SeqCst) != 1 {
                        log_ts!("Clipboard monitoring stopped");
                        break;
                    }

                    // Check if the clipboard has changed
                    let pasteboard = NSPasteboard::generalPasteboard();
                    let current_change_count = pasteboard.changeCount() as u64;
                    let change_count = &*(change_count_ptr as *const AtomicU64);
                    let stored_change_count = change_count.load(Ordering::SeqCst);

                    // If clipboard content has changed, update it
                    if current_change_count != stored_change_count {
                        log_ts!(
                            "Clipboard changed: {} -> {}",
                            stored_change_count,
                            current_change_count
                        );

                        change_count.store(current_change_count, Ordering::SeqCst);
                        let app_delegate = &*(app_delegate_ptr as *const AppDelegate);
                        app_delegate.update_text_from_clipboard();
                    }

                    // Sleep for 500ms before checking again
                    thread::sleep(Duration::from_millis(500));
                }
            });
        }

        log_ts!("Clipboard monitoring started");
    }
}

fn main() {
    log_ts!("Application starting");

    // Initialize on the main thread
    log_ts!("Creating MainThreadMarker");
    let mtm = MainThreadMarker::new().expect("Not running on main thread");
    log_ts!("MainThreadMarker created");

    // Get the shared application instance
    log_ts!("Getting shared application instance");
    let app = NSApplication::sharedApplication(mtm);
    log_ts!("Application instance acquired");

    // Set the activation policy to regular (creates a dock icon)
    log_ts!("Setting activation policy");
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    log_ts!("Activation policy set");

    // Create our app delegate
    log_ts!("Creating app delegate");
    let delegate = AppDelegate::new(mtm);
    log_ts!("App delegate created");

    // Set the delegate
    log_ts!("Setting application delegate");
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    log_ts!("Delegate set");

    log_ts!("Starting application run loop");
    app.run();
}
