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

static NSMutableArray<TollWindow *> *windows;
static NSMutableArray<NSTextField *> *labels;
static NSMutableArray<NSButton *> *buttons;
static bool newTab = false;
static bool visible = false;
static NSUInteger screenCount = 0;

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
        windows = [NSMutableArray new];
        labels = [NSMutableArray new];
        buttons = [NSMutableArray new];
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

bool tt_foreground(char *buffer, size_t size, uint32_t *pid) {
    @autoreleasepool {
        NSRunningApplication *app = NSWorkspace.sharedWorkspace.frontmostApplication;
        if (!app) return false;
        NSString *identifier = app.bundleIdentifier ?: app.executableURL.lastPathComponent;
        if (!identifier) return false;
        *pid = app.processIdentifier;
        return [identifier getCString:buffer maxLength:size encoding:NSUTF8StringEncoding];
    }
}

double tt_idle_seconds(void) {
    return CGEventSourceSecondsSinceLastEventType(kCGEventSourceStateCombinedSessionState, kCGAnyInputEventType);
}

static void createWindows(void) {
    for (NSWindow *window in windows) [window orderOut:nil];
    [windows removeAllObjects]; [labels removeAllObjects]; [buttons removeAllObjects];
    NSArray<NSScreen *> *screens = NSScreen.screens;
    screenCount = screens.count;
    for (NSScreen *screen in screens) {
        TollWindow *window = [[TollWindow alloc] initWithContentRect:screen.frame styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];
        window.releasedWhenClosed = NO;
        window.level = NSScreenSaverWindowLevel;
        window.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
        window.backgroundColor = [NSColor colorWithSRGBRed:0.07 green:0.09 blue:0.13 alpha:1];
        window.hidesOnDeactivate = NO;
        NSTextField *label = [NSTextField wrappingLabelWithString:@""];
        label.textColor = NSColor.whiteColor;
        label.font = [NSFont systemFontOfSize:25 weight:NSFontWeightMedium];
        label.alignment = NSTextAlignmentCenter;
        label.translatesAutoresizingMaskIntoConstraints = NO;
        [window.contentView addSubview:label];
        NSButton *button = [NSButton buttonWithTitle:@"Open a new browser tab" target:actions action:@selector(openTab:)];
        button.bezelStyle = NSBezelStyleRounded;
        button.translatesAutoresizingMaskIntoConstraints = NO;
        [window.contentView addSubview:button];
        [NSLayoutConstraint activateConstraints:@[
            [label.centerXAnchor constraintEqualToAnchor:window.contentView.centerXAnchor],
            [label.centerYAnchor constraintEqualToAnchor:window.contentView.centerYAnchor constant:-35],
            [label.widthAnchor constraintLessThanOrEqualToConstant:720],
            [label.widthAnchor constraintLessThanOrEqualToAnchor:window.contentView.widthAnchor multiplier:0.85],
            [button.centerXAnchor constraintEqualToAnchor:window.contentView.centerXAnchor],
            [button.topAnchor constraintEqualToAnchor:label.bottomAnchor constant:35]
        ]];
        [windows addObject:window]; [labels addObject:label]; [buttons addObject:button];
    }
}

void tt_show(const char *message, bool browser) {
    @autoreleasepool {
        bool rebuilt = screenCount != NSScreen.screens.count || windows.count == 0;
        if (rebuilt) createWindows();
        NSString *text = [NSString stringWithUTF8String:message] ?: @"TimeToll";
        for (NSUInteger i = 0; i < windows.count; ++i) {
            [windows[i] setFrame:NSScreen.screens[i].frame display:YES];
            labels[i].stringValue = text;
            buttons[i].hidden = !browser;
            [windows[i] orderFrontRegardless];
        }
        if (!visible || rebuilt || NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier != getpid()) {
            [NSApp activateIgnoringOtherApps:YES];
            [windows.firstObject makeKeyAndOrderFront:nil];
        }
        visible = true;
    }
}

void tt_hide(uint32_t restore_pid) {
    @autoreleasepool {
        if (!visible) return;
        for (NSWindow *window in windows) [window orderOut:nil];
        visible = false;
        if (restore_pid && NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier == getpid()) {
            [[NSRunningApplication runningApplicationWithProcessIdentifier:restore_pid] activateWithOptions:NSApplicationActivateIgnoringOtherApps];
        }
    }
}

bool tt_take_new_tab(void) { bool result = newTab; newTab = false; return result; }
