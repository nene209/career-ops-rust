use iced_test::simulator;
use iced_unlock_spike::UnlockApp;

#[test]
fn unlock_screen_headless() {
    let mut app = UnlockApp::new();
    let mut ui = simulator(app.view());

    // Focus the passphrase input. `typewrite` types into the focused
    // widget; without an explicit click nothing is focused, so the
    // call would be a no-op.
    ui.click("Passphrase").expect("click passphrase input");

    ui.typewrite("wrong");

    // Now that the heading is gone, "Unlock" matches only the button.
    ui.click("Unlock").expect("click unlock button");

    for message in ui.into_messages() {
        let _ = app.update(message);
    }

    assert_eq!(
        app.error,
        Some("Incorrect passphrase".into()),
        "unlock with wrong passphrase should set the error"
    );

    // Rebuild the view and assert the error is displayed.
    let mut ui = simulator(app.view());
    assert!(
        ui.find("Incorrect passphrase").is_ok(),
        "error message should be visible in the rendered UI"
    );
}
