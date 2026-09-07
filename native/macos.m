#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#include <stdint.h>
#include <stdbool.h>

@interface TollWindow : NSWindow
@end
@implementation TollWindow
- (BOOL)canBecomeKeyWindow { return YES; }
- (BOOL)canBecomeMainWindow { return YES; }
@end

static TollWindow *overlay;
static NSTextField *label;
static NSButton *button;
static bool newTab = false;
static bool visible = false;


@interface TollActions : NSObject
- (void)openTab:(id)sender;
@end
@implementation TollActions
- (void)openTab:(id)sender { newTab = true; }
@end
static TollActions *actions;

bool tt_init(void) {
    @autoreleasepool {
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        [NSApp finishLaunching];
        actions = [TollActions new];
        return [NSScreen screens].count > 0;
    }
}

void tt_pump(void) {
    @autoreleasepool {
        NSEvent *event;
        while ((event = [NSApp nextEventMatchingMask:NSEventMaskAny untilDate:[NSDate distantPast] inMode:NSDefaultRunLoopMode dequeue:YES])) {
            [NSApp sendEvent:event];
        }
        [NSApp updateWindows];
    }
}

// This reads public window geometry, never window titles or screen images.
static NSArray<NSDictionary *> *onScreenWindows(void) {
    return CFBridgingRelease(CGWindowListCopyWindowInfo(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID));
}

static bool windowBounds(NSDictionary *info, uint32_t pid, CGRect *bounds) {
    return [info[(id)kCGWindowOwnerPID] unsignedIntValue] == pid
        && [info[(id)kCGWindowLayer] intValue] == 0
        && [info[(id)kCGWindowAlpha] doubleValue] > 0
        && CGRectMakeWithDictionaryRepresentation((__bridge CFDictionaryRef)info[(id)kCGWindowBounds], bounds)
        && isfinite(bounds->origin.x) && isfinite(bounds->origin.y)
        && isfinite(bounds->size.width) && isfinite(bounds->size.height)
        && bounds->size.width > 0 && bounds->size.height > 0;
}

bool tt_foreground(char *buffer, size_t size, uint32_t *pid, uint64_t *window_id) {
    @autoreleasepool {
        NSRunningApplication *app = NSWorkspace.sharedWorkspace.frontmostApplication;
        if (!app) return false;
        NSString *identifier = app.bundleIdentifier ?: app.executableURL.lastPathComponent;
        if (!identifier) return false;
        *pid = app.processIdentifier;
        *window_id = 0;
        // The list is in front-to-back order. Keep the window ID, not just its
        // owner, so another window from the same app cannot become the target
        // while our overlay has focus.
        for (NSDictionary *info in onScreenWindows()) {
            CGRect bounds;
            if (windowBounds(info, *pid, &bounds)) {
                *window_id = [info[(id)kCGWindowNumber] unsignedLongLongValue];
                break;
            }
        }
        return [identifier getCString:buffer maxLength:size encoding:NSUTF8StringEncoding];
    }
}

static NSRect appKitFrame(CGRect bounds, CGFloat primaryHeight) {
    return NSMakeRect(bounds.origin.x, primaryHeight - CGRectGetMaxY(bounds),
                      bounds.size.width, bounds.size.height);
}

static bool targetFrame(uint64_t window_id, uint32_t pid, NSRect *frame) {
    if (!window_id) return false;
    for (NSDictionary *info in onScreenWindows()) {
        if ([info[(id)kCGWindowNumber] unsignedLongLongValue] != window_id) continue;
        CGRect bounds;
        if (!windowBounds(info, pid, &bounds)) return false;
        // Quartz uses a top-left origin; AppKit uses a bottom-left origin on
        // the primary screen. Do not flip relative to the target's monitor.
        CGFloat primaryHeight = CGDisplayBounds(CGMainDisplayID()).size.height;
        *frame = appKitFrame(bounds, primaryHeight);
        return true;
    }
    return false;
}

double tt_idle_seconds(void) {
    return CGEventSourceSecondsSinceLastEventType(kCGEventSourceStateCombinedSessionState, kCGAnyInputEventType);
}

static void createOverlay(void) {
    overlay = [[TollWindow alloc] initWithContentRect:NSMakeRect(0, 0, 1, 1)
        styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];
    overlay.releasedWhenClosed = NO;
    overlay.animationBehavior = NSWindowAnimationBehaviorNone;
    overlay.level = NSModalPanelWindowLevel;
    overlay.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
    overlay.backgroundColor = [NSColor colorWithSRGBRed:0.07 green:0.09 blue:0.13 alpha:1];
    overlay.hidesOnDeactivate = NO;
    label = [NSTextField wrappingLabelWithString:@""];
    label.textColor = NSColor.whiteColor;
    label.alignment = NSTextAlignmentCenter;
    [overlay.contentView addSubview:label];
    button = [NSButton buttonWithTitle:@"Open a new browser tab" target:actions action:@selector(openTab:)];
    button.bezelStyle = NSBezelStyleRounded;
    [overlay.contentView addSubview:button];
}

void tt_hide(uint32_t restore_pid);

bool tt_show(uint64_t window_id, uint32_t pid, const char *message, bool browser) {
    @autoreleasepool {
        NSRect frame;
        if (!targetFrame(window_id, pid, &frame)) {
            tt_hide(0);
            return false;
        }
        if (!overlay) createOverlay();
        [overlay setFrame:frame display:YES];
        // Fit the controls to small windows as well as fullscreen windows.
        CGFloat width = frame.size.width;
        CGFloat height = frame.size.height;
        CGFloat fontSize = MAX(11, MIN(25, MIN(width / 28, height / 21)));
        label.font = [NSFont systemFontOfSize:fontSize weight:NSFontWeightMedium];
        label.stringValue = [NSString stringWithUTF8String:message] ?: @"TimeToll";
        CGFloat buttonHeight = MIN(32, height / 5);
        CGFloat padding = MIN(16, MIN(width, height) / 10);
        CGFloat reserved = browser ? buttonHeight + padding : 0;
        CGFloat textWidth = MIN(720, MAX(1, width - 2 * padding));
        NSSize textSize = [label.cell cellSizeForBounds:NSMakeRect(0, 0, textWidth, CGFLOAT_MAX)];
        CGFloat textHeight = MIN(textSize.height, MAX(1, height - 2 * padding - reserved));
        CGFloat textBottom = MAX(padding + reserved, (height - textHeight + reserved) / 2);
        label.frame = NSMakeRect((width - textWidth) / 2, textBottom, textWidth, textHeight);
        CGFloat buttonWidth = MIN(260, MAX(1, width - 2 * padding));
        button.frame = NSMakeRect((width - buttonWidth) / 2, MAX(padding, textBottom - reserved), buttonWidth, buttonHeight);
        button.hidden = !browser;
        [overlay orderFrontRegardless];
        if (!visible || NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier != getpid()) {
            [NSApp activateIgnoringOtherApps:YES];
            [overlay makeKeyAndOrderFront:nil];
        }
        visible = true;
        return true;
    }
}

void tt_hide(uint32_t restore_pid) {
    @autoreleasepool {
        if (!visible) return;
        [overlay orderOut:nil];
        visible = false;
        if (restore_pid && NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier == getpid()) {
            [[NSRunningApplication runningApplicationWithProcessIdentifier:restore_pid] activateWithOptions:NSApplicationActivateIgnoringOtherApps];
        }
    }
}

bool tt_take_new_tab(void) { bool result = newTab; newTab = false; return result; }
