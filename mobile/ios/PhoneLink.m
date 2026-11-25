#import "PhoneLink.h"

@interface PhoneLink () <NSURLSessionTaskDelegate>
@property (strong) NSURLSession *session;
@property (strong) NSURL *root;
@property (copy) NSString *origin;
@end

@implementation PhoneLink
- (instancetype)initWithHost:(NSString *)host {
    if ((self = [super init])) {
        self.origin = [NSString stringWithFormat:@"https://%@:47745", host];
        self.root = [NSURL URLWithString:[self.origin stringByAppendingString:@"/"]];
        NSURLSessionConfiguration *config = NSURLSessionConfiguration.ephemeralSessionConfiguration;
        config.requestCachePolicy = NSURLRequestReloadIgnoringLocalCacheData;
        config.timeoutIntervalForRequest = 10;
        self.session = [NSURLSession sessionWithConfiguration:config delegate:self delegateQueue:nil];
    }
    return self;
}

- (void)close {
    [self.session finishTasksAndInvalidate];
}

- (NSMutableURLRequest *)requestForURL:(NSURL *)url {
    NSMutableURLRequest *request = [NSMutableURLRequest requestWithURL:url];
    [request setValue:self.origin forHTTPHeaderField:@"Origin"];
    return request;
}

- (void)request:(NSString *)path body:(NSDictionary *)body stream:(NSString *)stream completion:(PhoneReply)reply {
    NSMutableURLRequest *request = [self requestForURL:[NSURL URLWithString:path relativeToURL:self.root].absoluteURL];
    if (body || stream) {
        request.HTTPMethod = @"POST";
        if (body) {
            request.HTTPBody = [NSJSONSerialization dataWithJSONObject:body options:0 error:NULL];
            [request setValue:@"application/json" forHTTPHeaderField:@"Content-Type"];
        }
        if (stream) [request setValue:stream forHTTPHeaderField:@"X-PocketCam-Stream"];
    }
    [[self.session dataTaskWithRequest:request completionHandler:^(NSData *data, NSURLResponse *response, NSError *error) {
        NSDictionary *json = nil;
        if (data.length) {
            id parsed = [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL];
            if ([parsed isKindOfClass:NSDictionary.class]) json = parsed;
        }
        NSInteger status = [(NSHTTPURLResponse *)response statusCode];
        if (reply) dispatch_async(dispatch_get_main_queue(), ^{ reply(status, json, error); });
    }] resume];
}

- (NSURLSessionWebSocketTask *)socketForStream:(NSString *)stream {
    NSURLComponents *components = [NSURLComponents componentsWithURL:self.root resolvingAgainstBaseURL:NO];
    components.scheme = @"wss";
    components.path = @"/api/video";
    components.queryItems = @[[NSURLQueryItem queryItemWithName:@"stream" value:stream]];
    NSMutableURLRequest *request = [self requestForURL:components.URL];
    NSArray *cookies = [self.session.configuration.HTTPCookieStorage cookiesForURL:self.root];
    NSString *cookie = [NSHTTPCookie requestHeaderFieldsWithCookies:cookies ?: @[]][@"Cookie"];
    if (cookie) [request setValue:cookie forHTTPHeaderField:@"Cookie"];
    return [self.session webSocketTaskWithRequest:request];
}

- (void)URLSession:(NSURLSession *)session task:(NSURLSessionTask *)task
    willPerformHTTPRedirection:(NSHTTPURLResponse *)response
    newRequest:(NSURLRequest *)request completionHandler:(void (^)(NSURLRequest *))completion {
    completion(nil);
}
@end
