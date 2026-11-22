mod app;
mod catalog;
mod guides;
mod launcher;
mod results;
mod snap;

use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    let mtm = MainThreadMarker::new().expect("rust-overlay must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // Accessory apps get no Dock icon or app menu and never steal focus from the app below.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // NSApplication only holds a weak reference to its delegate.
    let delegate = app::AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
