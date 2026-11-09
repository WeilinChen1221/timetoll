#import <AppKit/AppKit.h>
#include <stdbool.h>

// A nonactivating panel keeps the foreground app earning/spending while the
// status is visible. NSMenu tracking would block the monitor's event pump.
@interface TollStatusPanel : NSPanel
@end
@implementation TollStatusPanel
- (BOOL)canBecomeKeyWindow { return NO; }
- (BOOL)canBecomeMainWindow { return NO; }
@end

@interface TollTray : NSObject
@property NSStatusItem *item;
@property TollStatusPanel *panel;
@property NSArray<NSTextField *> *rows;
@property id globalClick;
@property id localClick;
- (void)toggle:(id)sender;
- (void)dismissOutside;
- (void)positionPanel;
@end

@implementation TollTray
- (void)positionPanel {
    NSRect anchor = [self.item.button.window convertRectToScreen:
        [self.item.button convertRect:self.item.button.bounds toView:nil]];
    NSRect screen = self.item.button.window.screen.visibleFrame;
    NSRect frame = self.panel.frame;
    frame.origin.x = MAX(NSMinX(screen), MIN(NSMidX(anchor) - frame.size.width / 2,
                                            NSMaxX(screen) - frame.size.width));
    frame.origin.y = NSMinY(anchor) - frame.size.height - 6;
    [self.panel setFrame:frame display:YES];
}
- (void)toggle:(id)sender {
    if (self.panel.visible) {
        [self.panel orderOut:nil];
    } else {
        [self positionPanel];
        [self.panel orderFrontRegardless];
    }
}
- (void)dismissOutside {
    if (!self.panel.visible) return;
    NSPoint mouse = NSEvent.mouseLocation;
    NSRect anchor = [self.item.button.window convertRectToScreen:
        [self.item.button convertRect:self.item.button.bounds toView:nil]];
    if (!NSPointInRect(mouse, self.panel.frame) && !NSPointInRect(mouse, anchor)) {
        [self.panel orderOut:nil];
    }
}
@end

static TollTray *tray;

bool tt_tray_init(void) {
    @autoreleasepool {
        tray = [TollTray new];
        tray.item = [NSStatusBar.systemStatusBar statusItemWithLength:NSSquareStatusItemLength];
        if (!tray.item.button) { tray = nil; return false; }
        NSImage *icon = [[NSImage alloc] initWithSize:NSMakeSize(18, 18)];
        [icon lockFocus];
        NSBezierPath *path = [NSBezierPath bezierPath];
        path.lineWidth = 1.6;
        path.lineCapStyle = NSLineCapStyleRound;
        [path moveToPoint:NSMakePoint(4, 16)];
        [path lineToPoint:NSMakePoint(14, 16)];
        [path moveToPoint:NSMakePoint(4, 2)];
        [path lineToPoint:NSMakePoint(14, 2)];
        [path moveToPoint:NSMakePoint(5, 15)];
        [path curveToPoint:NSMakePoint(13, 3) controlPoint1:NSMakePoint(5, 8) controlPoint2:NSMakePoint(13, 10)];
        [path moveToPoint:NSMakePoint(13, 15)];
        [path curveToPoint:NSMakePoint(5, 3) controlPoint1:NSMakePoint(13, 8) controlPoint2:NSMakePoint(5, 10)];
        [NSColor.blackColor setStroke];
        [path stroke];
        [icon unlockFocus];
        icon.template = YES;
        tray.item.button.image = icon;
        tray.item.button.toolTip = @"TimeToll";
        [tray.item.button setAccessibilityLabel:@"TimeToll status"];
        tray.item.button.target = tray;
        tray.item.button.action = @selector(toggle:);
        [tray.item.button sendActionOn:NSEventMaskLeftMouseUp | NSEventMaskRightMouseUp];

        tray.panel = [[TollStatusPanel alloc] initWithContentRect:NSMakeRect(0, 0, 340, 152)
            styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
            backing:NSBackingStoreBuffered defer:NO];
        tray.panel.releasedWhenClosed = NO;
        tray.panel.hidesOnDeactivate = NO;
        tray.panel.level = NSStatusWindowLevel;
        tray.panel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
        tray.panel.hasShadow = YES;
        tray.panel.opaque = NO;
        tray.panel.backgroundColor = NSColor.clearColor;
        NSVisualEffectView *content = [[NSVisualEffectView alloc] initWithFrame:tray.panel.contentView.bounds];
        content.material = NSVisualEffectMaterialPopover;
        content.state = NSVisualEffectStateActive;
        content.wantsLayer = YES;
        content.layer.cornerRadius = 10;
        content.layer.masksToBounds = YES;
        tray.panel.contentView = content;
        NSMutableArray *rows = [NSMutableArray array];
        for (NSUInteger i = 0; i < 4; i++) {
            NSTextField *row = [NSTextField labelWithString:i == 0 ? @"TimeToll" : @""];
            row.font = i == 0 ? [NSFont systemFontOfSize:14 weight:NSFontWeightSemibold]
                : [NSFont monospacedDigitSystemFontOfSize:13 weight:NSFontWeightRegular];
            row.textColor = i == 0 ? NSColor.secondaryLabelColor : NSColor.labelColor;
            row.frame = NSMakeRect(18, 117 - i * 29, 304, 20);
            [content addSubview:row];
            [rows addObject:row];
        }
        tray.rows = rows;
        // Mouse-only global monitors need no Accessibility permission.
        __weak TollTray *weakTray = tray;
        NSEventMask clicks = NSEventMaskLeftMouseDown | NSEventMaskRightMouseDown;
        tray.globalClick = [NSEvent addGlobalMonitorForEventsMatchingMask:clicks handler:^(NSEvent *event) {
            (void)event;
            [weakTray dismissOutside];
        }];
        tray.localClick = [NSEvent addLocalMonitorForEventsMatchingMask:clicks handler:^NSEvent *(NSEvent *event) {
            [weakTray dismissOutside];
            return event;
        }];
        return true;
    }
}

void tt_tray_update(const char *state, const char *balance, const char *progress) {
    @autoreleasepool {
        if (!tray) return;
        NSArray *values = @[[NSString stringWithUTF8String:state],
            [NSString stringWithUTF8String:balance], [NSString stringWithUTF8String:progress]];
        CGFloat width = 340;
        for (NSUInteger i = 0; i < values.count; i++) {
            NSTextField *row = tray.rows[i + 1];
            row.stringValue = values[i];
            width = MAX(width, row.cell.cellSize.width + 36);
        }
        [tray.panel setContentSize:NSMakeSize(width, 152)];
        for (NSTextField *row in tray.rows) {
            NSRect frame = row.frame;
            frame.size.width = width - 36;
            row.frame = frame;
        }
        if (tray.panel.visible) [tray positionPanel];
        tray.item.button.toolTip = [@"TimeToll\n" stringByAppendingString:[values componentsJoinedByString:@"\n"]];
    }
}

void tt_tray_destroy(void) {
    @autoreleasepool {
        if (!tray) return;
        if (tray.globalClick) [NSEvent removeMonitor:tray.globalClick];
        if (tray.localClick) [NSEvent removeMonitor:tray.localClick];
        [tray.panel close];
        [NSStatusBar.systemStatusBar removeStatusItem:tray.item];
        tray = nil;
    }
}
