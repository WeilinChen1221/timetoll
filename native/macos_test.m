// Opt-in desktop test. Creates and changes only this test process's windows.
#import "macos.m"
#include <assert.h>
#include <stdio.h>

static void settle(double seconds) {
    NSDate *end = [NSDate dateWithTimeIntervalSinceNow:seconds];
    while (end.timeIntervalSinceNow > 0) {
        tt_pump();
        [NSThread sleepForTimeInterval:0.01];
    }
}

static void checkOverlay(NSWindow *target, bool browser) {
    settle(0.5);
    assert(tt_show(target.windowNumber, getpid(), "TimeToll\n\nWindow bounds test", browser));
    settle(0.1);
    NSRect expected;
    assert(targetFrame(target.windowNumber, getpid(), &expected));
    if (!NSEqualRects(overlay.frame, expected)) NSLog(@"Geometry mismatch: overlay=%@ expected=%@ target=%@", NSStringFromRect(overlay.frame), NSStringFromRect(expected), NSStringFromRect(target.frame));
    assert(NSEqualRects(overlay.frame, expected));
    assert(overlay.isVisible);
    assert(NSContainsRect(overlay.contentView.bounds, label.frame));
    if (browser) assert(NSContainsRect(overlay.contentView.bounds, button.frame));
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        assert(tt_init());
        // Screens above, below, and left of the primary display retain the
        // global origin; conversion must not use a secondary screen's height.
        assert(NSEqualRects(appKitFrame(CGRectMake(-800, 100, 500, 300), 900), NSMakeRect(-800, 500, 500, 300)));
        assert(NSEqualRects(appKitFrame(CGRectMake(0, -600, 500, 300), 900), NSMakeRect(0, 1200, 500, 300)));
        assert(NSEqualRects(appKitFrame(CGRectMake(0, 1000, 500, 300), 900), NSMakeRect(0, -400, 500, 300)));
        NSRect area = NSScreen.mainScreen.visibleFrame;
        NSWindow *target = [[NSWindow alloc] initWithContentRect:NSMakeRect(area.origin.x + 60, area.origin.y + 80, 480, 360)
            styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable
            backing:NSBackingStoreBuffered defer:NO];
        target.releasedWhenClosed = NO;
        target.title = @"TimeToll test target";
        target.collectionBehavior = NSWindowCollectionBehaviorFullScreenPrimary;
        NSWindow *other = [[NSWindow alloc] initWithContentRect:NSMakeRect(area.origin.x + 620, area.origin.y + 80, 200, 200)
            styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
        other.releasedWhenClosed = NO;
        [other orderFrontRegardless];
        [target makeKeyAndOrderFront:nil];
        [NSApp activateIgnoringOtherApps:YES];
        settle(0.5);
        char app[1024];
        uint32_t owner = 0;
        uint64_t foregroundWindow = 0;
        assert(tt_foreground(app, sizeof(app), &owner, &foregroundWindow));
        assert(owner == getpid() && foregroundWindow == target.windowNumber);
        checkOverlay(target, true);
        assert(!NSEqualRects(overlay.frame, other.frame));
        [target setFrame:NSMakeRect(area.origin.x + 120, area.origin.y + 120, 320, 220) display:YES];
        checkOverlay(target, true);
        [target setFrame:NSScreen.mainScreen.frame display:YES];
        checkOverlay(target, false);
        [target setFrame:NSMakeRect(area.origin.x + 60, area.origin.y + 80, 480, 360) display:YES];
        checkOverlay(target, false);
        if (argc > 1 && strcmp(argv[1], "--fullscreen") == 0) {
            tt_hide(getpid());
            [target makeKeyAndOrderFront:nil];
            [target toggleFullScreen:nil];
            settle(2);
            assert((target.styleMask & NSWindowStyleMaskFullScreen) != 0);
            checkOverlay(target, false);
            tt_hide(getpid());
            [target toggleFullScreen:nil];
            settle(2);
            checkOverlay(target, false);
        }
        // Track this exact window even when another same-process window exists.
        uint64_t identifier = target.windowNumber;
        [target orderOut:nil];
        settle(0.5);
        assert(!tt_show(identifier, getpid(), "hidden", false));
        assert(!overlay.isVisible);
        [target orderFrontRegardless];
        checkOverlay(target, true);
        [target miniaturize:nil];
        settle(0.5);
        assert(!tt_show(identifier, getpid(), "minimized", false));
        assert(!overlay.isVisible);
        [target deminiaturize:nil];
        settle(0.5);
        checkOverlay(target, true);
        [target close];
        settle(0.5);
        assert(!tt_show(identifier, getpid(), "closed", false));
        assert(!overlay.isVisible);
        assert(!tt_show(other.windowNumber, getpid() + 1, "wrong owner", false));
        [other close];
        puts("PASS: window identity, movement, resize, screen-sized bounds, hiding, minimization, closure, and coordinate conversion");
    }
}
