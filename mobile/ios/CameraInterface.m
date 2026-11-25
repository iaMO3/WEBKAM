#import "CameraInterface.h"
#import <WebKit/WebKit.h>
#import <math.h>

@class CameraInterface;

@interface CameraMessages : NSObject <WKScriptMessageHandler>
@property (weak) CameraInterface *owner;
@end

@interface CameraInterface () <WKNavigationDelegate>
@property (strong) WKWebView *webView;
@property (strong) NSURL *page;
@property (strong) UIView *preview;
@property (strong) UIView *previewClip;
@property (strong) NSDictionary *state;
@property (copy) CameraInterfaceAction action;
@property (assign) BOOL ready;
@property (assign) CGRect stage;
@property (assign) CGFloat viewportWidth;
- (void)receive:(WKScriptMessage *)message;
@end

@implementation CameraMessages
- (void)userContentController:(WKUserContentController *)controller didReceiveScriptMessage:(WKScriptMessage *)message {
    [self.owner receive:message];
}
@end

@implementation CameraInterface
- (instancetype)initWithPreview:(UIView *)preview action:(CameraInterfaceAction)action {
    if (!(self = [super initWithFrame:CGRectZero])) return nil;
    self.backgroundColor = UIColor.systemBackgroundColor;
    self.action = action;
    self.preview = preview;
    self.preview.userInteractionEnabled = NO;
    self.previewClip = [UIView new];
    self.previewClip.clipsToBounds = YES;
    self.previewClip.userInteractionEnabled = NO;
    self.previewClip.hidden = YES;

    CameraMessages *bridge = [CameraMessages new];
    bridge.owner = self;
    WKWebViewConfiguration *configuration = [WKWebViewConfiguration new];
    [configuration.userContentController addScriptMessageHandler:bridge name:@"pocketcam"];
    self.webView = [[WKWebView alloc] initWithFrame:CGRectZero configuration:configuration];
    self.webView.navigationDelegate = self;
    self.webView.scrollView.contentInsetAdjustmentBehavior = UIScrollViewContentInsetAdjustmentNever;
    self.webView.translatesAutoresizingMaskIntoConstraints = NO;
    [self addSubview:self.webView];
    [self addSubview:self.previewClip];
    [self.previewClip addSubview:self.preview];
    [NSLayoutConstraint activateConstraints:@[
        [self.webView.topAnchor constraintEqualToAnchor:self.topAnchor],
        [self.webView.bottomAnchor constraintEqualToAnchor:self.bottomAnchor],
        [self.webView.leadingAnchor constraintEqualToAnchor:self.leadingAnchor],
        [self.webView.trailingAnchor constraintEqualToAnchor:self.trailingAnchor]
    ]];

    self.page = [NSBundle.mainBundle URLForResource:@"camera" withExtension:@"html" subdirectory:@"CameraUI"];
    if (self.page) {
        [self.webView loadFileURL:self.page allowingReadAccessToURL:self.page.URLByDeletingLastPathComponent];
    } else {
        [self showLoadError];
    }
    return self;
}

- (void)showLoadError {
    UILabel *status = [UILabel new];
    status.translatesAutoresizingMaskIntoConstraints = NO;
    status.numberOfLines = 0;
    status.text = @"PocketCam’s camera screen couldn’t load. Reinstall the PocketCam PiP app.";
    [self addSubview:status];
    [NSLayoutConstraint activateConstraints:@[
        [status.centerYAnchor constraintEqualToAnchor:self.centerYAnchor],
        [status.leadingAnchor constraintEqualToAnchor:self.leadingAnchor constant:18],
        [status.trailingAnchor constraintEqualToAnchor:self.trailingAnchor constant:-18]
    ]];
}

- (void)webView:(WKWebView *)webView decidePolicyForNavigationAction:(WKNavigationAction *)action
    decisionHandler:(void (^)(WKNavigationActionPolicy))decision {
    BOOL localPage = action.targetFrame.mainFrame && [action.request.URL isEqual:self.page];
    decision(localPage ? WKNavigationActionPolicyAllow : WKNavigationActionPolicyCancel);
}

- (void)webView:(WKWebView *)webView didFailProvisionalNavigation:(WKNavigation *)navigation withError:(NSError *)error {
    if (error.code != NSURLErrorCancelled) [self showLoadError];
}

- (void)webViewWebContentProcessDidTerminate:(WKWebView *)webView {
    self.ready = NO;
    self.stage = CGRectZero;
    self.previewClip.hidden = YES;
    self.action(@"interface-reload", @{});
    if (self.page) [webView loadFileURL:self.page allowingReadAccessToURL:self.page.URLByDeletingLastPathComponent];
}

- (void)render:(NSDictionary *)state {
    self.state = state;
    [self positionPreview];
    if (!self.ready || !state) return;
    [self.webView callAsyncJavaScript:@"window.renderPocketCam(state)" arguments:@{@"state":state}
        inFrame:nil inContentWorld:WKContentWorld.pageWorld completionHandler:nil];
}

- (void)receive:(WKScriptMessage *)message {
    if (message.webView != self.webView || !message.frameInfo.mainFrame
        || ![message.frameInfo.request.URL isEqual:self.page]
        || ![message.body isKindOfClass:NSDictionary.class]) return;
    NSDictionary *payload = message.body;
    NSString *action = payload[@"action"];
    if (![action isKindOfClass:NSString.class]) return;
    if ([action isEqualToString:@"ready"]) {
        self.ready = YES;
        [self render:self.state];
    } else if ([action isEqualToString:@"layout"]) {
        [self updateStage:payload];
    } else if (self.ready && [@[@"pair", @"retry", @"lens", @"camera", @"pip"] containsObject:action]) {
        self.action(action,payload);
    }
}

- (void)updateStage:(NSDictionary *)payload {
    NSArray *keys = @[@"x", @"y", @"width", @"height", @"viewportWidth"];
    for (NSString *key in keys) {
        id value = payload[key];
        if (![value isKindOfClass:NSNumber.class] || !isfinite([value doubleValue])) return;
    }
    CGFloat width = [payload[@"width"] doubleValue], height = [payload[@"height"] doubleValue];
    CGFloat viewport = [payload[@"viewportWidth"] doubleValue];
    if (viewport < 1 || width < 0 || height < 0) return;
    self.stage = CGRectMake([payload[@"x"] doubleValue], [payload[@"y"] doubleValue], width, height);
    self.viewportWidth = viewport;
    [self positionPreview];
}

- (void)layoutSubviews {
    [super layoutSubviews];
    [self positionPreview];
}

- (void)positionPreview {
    if (![self.state[@"preview"] boolValue] || self.viewportWidth < 1 || CGRectIsEmpty(self.stage)) {
        self.previewClip.hidden = YES;
        return;
    }
    CGFloat scale = self.bounds.size.width / self.viewportWidth;
    CGRect frame = CGRectApplyAffineTransform(self.stage, CGAffineTransformMakeScale(scale,scale));
    CGRect clipped = CGRectIntersection(self.bounds, frame);
    if (CGRectIsEmpty(clipped)) {
        self.previewClip.hidden = YES;
        return;
    }
    self.previewClip.frame = clipped;
    self.preview.frame = CGRectOffset(frame, -clipped.origin.x, -clipped.origin.y);
    self.previewClip.hidden = NO;
}
@end
