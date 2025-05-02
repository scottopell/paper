#![deny(unsafe_op_in_unsafe_fn)]
#![allow(non_snake_case)]

use std::cell::OnceCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

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
            println!("Application did finish launching");

            let mtm = self.mtm();

            // Create and setup the window
            let window = self.create_window(mtm);
            let _ = self.ivars().window.set(window.clone());

            window.setTitle(ns_string!("Clipboard Viewer"));
            window.center();

            // Create and setup the text field
            self.setup_text_field(&window, mtm);

            // Initialize the change count with the current value
            let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
            let initial_change_count = unsafe { pasteboard.changeCount() };
            self.ivars()
                .change_count
                .store(initial_change_count as u64, Ordering::SeqCst);

            // Set flag to start checking clipboard
            self.ivars()
                .should_check_clipboard
                .store(1, Ordering::SeqCst);

            // Start clipboard check loop
            self.start_clipboard_check_loop();

            // Activate app and make window visible
            let app = NSApplication::sharedApplication(mtm);
            unsafe { app.activate() };
            window.makeKeyAndOrderFront(None);
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowWillClose:))]
        fn windowWillClose(&self, _notification: &NSNotification) {
            // Stop the clipboard check loop
            self.ivars()
                .should_check_clipboard
                .store(0, Ordering::SeqCst);

            let mtm = self.mtm();
            let app = NSApplication::sharedApplication(mtm);
            unsafe { app.terminate(None) };
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars::default());
        unsafe { msg_send![super(this), init] }
    }

    fn create_window(&self, mtm: MainThreadMarker) -> Retained<NSWindow> {
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

        // Set delegate to handle window close
        unsafe {
            window.setDelegate(Some(ProtocolObject::from_ref(self)));
            window.setReleasedWhenClosed(false);
        }

        window
    }

    fn setup_text_field(&self, window: &NSWindow, mtm: MainThreadMarker) {
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

        unsafe {
            // Configure text field properties
            text_field.setEditable(false);
            text_field.setBezeled(false);
            text_field.setDrawsBackground(false);
            text_field.setSelectable(true); // Allow selecting text for copying
            text_field.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );

            // Set an initial value
            text_field.setStringValue(ns_string!("Monitoring clipboard... Copy something!"));

            // Add to content view
            content_view.addSubview(&text_field);

            // Store the text field
            let _ = self.ivars().text_field.set(text_field);
        }
    }

    fn start_clipboard_check_loop(&self) {
        // Create a copy of self for the background thread
        let delegate_ptr = self as *const _ as usize;

        // Create copies of the atomic values to check in another thread
        let should_check_ptr = &self.ivars().should_check_clipboard as *const _ as usize;
        let change_count_ptr = &self.ivars().change_count as *const _ as usize;
        let text_field_ptr = &self.ivars().text_field as *const _ as usize;

        // Spawn a thread to check the clipboard
        thread::spawn(move || {
            // Sleep a bit to allow the UI to initialize
            thread::sleep(Duration::from_millis(500));

            while unsafe {
                // Access the atomic flag to see if we should continue checking
                let should_check = &*(should_check_ptr as *const AtomicU64);
                should_check.load(Ordering::SeqCst) == 1
            } {
                // Get current change count
                let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
                let current_change_count = unsafe { pasteboard.changeCount() } as u64;

                // Access the stored change count
                let stored_change_count = unsafe {
                    let change_count = &*(change_count_ptr as *const AtomicU64);
                    change_count.load(Ordering::SeqCst)
                };

                // Only update if the clipboard has changed
                if current_change_count != stored_change_count {
                    // Update the stored change count
                    unsafe {
                        let change_count = &*(change_count_ptr as *const AtomicU64);
                        change_count.store(current_change_count, Ordering::SeqCst);
                    }

                    // Update the text field on the main thread
                    unsafe {
                        let text_field_cell =
                            &*(text_field_ptr as *const OnceCell<Retained<NSTextField>>);
                        if let Some(text_field) = text_field_cell.get() {
                            // Set default message
                            let message = ns_string!("Clipboard contains non-text content");

                            // Try to get text from the pasteboard
                            let string_type = ns_string!("public.utf8-plain-text");
                            if let Some(clipboard_text) = pasteboard.stringForType(string_type) {
                                text_field.setStringValue(&clipboard_text);
                            } else {
                                text_field.setStringValue(message);
                            }
                        }
                    }
                }

                // Sleep for a short duration before checking again
                thread::sleep(Duration::from_millis(500));
            }
        });
    }
}

fn main() {
    // Initialize on the main thread
    let mtm = MainThreadMarker::new().expect("Not running on main thread");

    // Get the shared application instance
    let app = NSApplication::sharedApplication(mtm);

    // Set the activation policy to regular (creates a dock icon)
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    // Create our app delegate
    let delegate = AppDelegate::new(mtm);

    // Set the delegate
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    println!("Starting application run loop");
    app.run();
}
