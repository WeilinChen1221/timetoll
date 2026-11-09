// Opt-in desktop test. Uses only this process's windows and status item.
#import "macos.m"
#import "macos_tray.m"
#include <assert.h>
#include <stdio.h>

int main(void) {
    @autoreleasepool {
        assert(tt_init());
        assert(tt_tray_init());
        assert(tray.item.button.image.template);
        tt_tray_update("State: earning", "Banked access: 0m 0s", "Earning progress: 0m 0s / 15m 0s");
        NSWindow *keyBefore = NSApp.keyWindow;
        pid_t foregroundBefore = NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier;
        [tray toggle:nil];
        assert(tray.panel.visible);
        NSDate *start = [NSDate date];
        // A menu would block inside the event pump while open. The panel must
        // let monitoring and live status updates continue without taking focus.
        for (int i = 0; i < 5; i++) tt_pump();
        assert(-start.timeIntervalSinceNow < 1);
        assert(NSApp.keyWindow == keyBefore);
        assert(NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier == foregroundBefore);
        tt_tray_update("State: spending", "Banked access: 9m 59s", "Earning progress: 0m 0s / 15m 0s");
        assert([tray.rows[1].stringValue isEqualToString:@"State: spending"]);
        assert([tray.rows[2].stringValue isEqualToString:@"Banked access: 9m 59s"]);
        tt_tray_update("State: waiting for browser extension", "Banked access: 307445734561825m 51s", "Earning progress: 525599m 59s / 525600m 0s");
        for (NSTextField *row in tray.rows) {
            assert(row.cell.cellSize.width <= row.frame.size.width);
            assert(NSContainsRect(tray.panel.contentView.bounds, row.frame));
        }
        tt_tray_update("State: earning", "Banked access: 0m 0s", "Earning progress: 0m 0s / 15m 0s");
        [tray.panel.contentView display];
        NSView *view = tray.panel.contentView;
        NSBitmapImageRep *bitmap = [view bitmapImageRepForCachingDisplayInRect:view.bounds];
        [view cacheDisplayInRect:view.bounds toBitmapImageRep:bitmap];
        [[bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}]
            writeToFile:@"target/macos-tray-preview.png" atomically:YES];
        [tray toggle:nil];
        assert(!tray.panel.visible);
        tt_tray_destroy();
        assert(tray == nil);
        // Reinitialization must not retain the previous item or mouse monitors.
        assert(tt_tray_init());
        tt_tray_destroy();
        puts("PASS: live tray updates, focus preservation, nonblocking pump, text layout, toggle, and cleanup");
    }
}
