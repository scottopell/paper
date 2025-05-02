#![deny(unsafe_op_in_unsafe_fn)]
#![allow(non_snake_case)]

use std::cell::OnceCell;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSTextField, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    ns_string, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
};

#[derive(Debug, Default)]
struct AppDelegateIvars {
    window: OnceCell<Retained<NSWindow>>,
    text_field: OnceCell<Retained<NSTextField>>,
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

            window.setTitle(ns_string!("Simple Window"));
            window.center();

            // Create and setup the text field
            self.setup_text_field(&window, mtm);

            // Activate app and make window visible
            let app = NSApplication::sharedApplication(mtm);
            unsafe { app.activate() };
            window.makeKeyAndOrderFront(None);
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowWillClose:))]
        fn windowWillClose(&self, _notification: &NSNotification) {
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
            text_field.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );

            // Set an initial value
            text_field.setStringValue(ns_string!("Hello World!"));

            // Add to content view
            content_view.addSubview(&text_field);

            // Store the text field
            let _ = self.ivars().text_field.set(text_field);
        }
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
