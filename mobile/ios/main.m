#import <UIKit/UIKit.h>
#import <WebKit/WebKit.h>
#import <arpa/inet.h>
#import "NativeCamera.h"

@interface ConnectionController : UIViewController <WKNavigationDelegate>
@property (strong) WKWebView *web;
@property (strong) NSURL *homeURL;
@end

@implementation ConnectionController
- (void)viewDidLoad {
    [super viewDidLoad];
    self.title = @"WEBKAM";
    self.view.backgroundColor = UIColor.systemBackgroundColor;
    self.web = [[WKWebView alloc] initWithFrame:CGRectZero configuration:[WKWebViewConfiguration new]];
    self.web.navigationDelegate = self;
    self.web.translatesAutoresizingMaskIntoConstraints = NO;
    [self.view addSubview:self.web];
    UILayoutGuide *safe = self.view.safeAreaLayoutGuide;
    [NSLayoutConstraint activateConstraints:@[
        [self.web.topAnchor constraintEqualToAnchor:safe.topAnchor],
        [self.web.bottomAnchor constraintEqualToAnchor:safe.bottomAnchor],
        [self.web.leadingAnchor constraintEqualToAnchor:safe.leadingAnchor],
        [self.web.trailingAnchor constraintEqualToAnchor:safe.trailingAnchor]
    ]];
    self.homeURL = [NSBundle.mainBundle URLForResource:@"home" withExtension:@"html" subdirectory:@"CameraUI"];
    [self.web loadFileURL:self.homeURL allowingReadAccessToURL:self.homeURL.URLByDeletingLastPathComponent];
}
static BOOL privateHost(NSString *host) {
    struct in_addr ip;
    if (!host || inet_pton(AF_INET, host.UTF8String, &ip) != 1) return NO;
    uint32_t address = ntohl(ip.s_addr);
    return (address >> 24) == 10 || (address >> 16) == 0xc0a8 || (address >> 20) == 0xac1;
}
- (void)webView:(WKWebView *)web decidePolicyForNavigationAction:(WKNavigationAction *)action
    decisionHandler:(void (^)(WKNavigationActionPolicy))decide {
    NSURL *url = action.request.URL;
    if ([url isEqual:self.homeURL]) { decide(WKNavigationActionPolicyAllow); return; }
    if (action.targetFrame.mainFrame && [web.URL isEqual:self.homeURL] && [url.scheme isEqualToString:@"pocketcam"]) {
        decide(WKNavigationActionPolicyCancel);
        NSString *ip = url.lastPathComponent;
        if (!privateHost(ip)) return;
        if ([url.host isEqualToString:@"setup"]) {
            NSURL *setup = [NSURL URLWithString:[NSString stringWithFormat:@"http://%@:47744/", ip]];
            [UIApplication.sharedApplication openURL:setup options:@{} completionHandler:nil];
        } else if ([url.host isEqualToString:@"camera"]) {
            [self.navigationController pushViewController:[[NativeCamera alloc] initWithHost:ip] animated:YES];
        }
        return;
    }
    decide(WKNavigationActionPolicyCancel);
}
@end

@interface PocketCamDelegate : UIResponder <UIApplicationDelegate>
@property (strong, nonatomic) UIWindow *window;
@end
@implementation PocketCamDelegate
- (BOOL)application:(UIApplication *)app didFinishLaunchingWithOptions:(NSDictionary *)options {
    self.window = [[UIWindow alloc] initWithFrame:UIScreen.mainScreen.bounds];
    self.window.rootViewController = [[UINavigationController alloc] initWithRootViewController:[ConnectionController new]];
    [self.window makeKeyAndVisible];
    return YES;
}
@end

int main(int argc, char *argv[]) {
    @autoreleasepool { return UIApplicationMain(argc, argv, nil, NSStringFromClass(PocketCamDelegate.class)); }
}
