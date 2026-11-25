#import <Foundation/Foundation.h>

typedef void (^PhoneReply)(NSInteger status, NSDictionary *json, NSError *error);

@interface PhoneLink : NSObject
- (instancetype)initWithHost:(NSString *)host;
- (void)request:(NSString *)path body:(NSDictionary *)body stream:(NSString *)stream completion:(PhoneReply)reply;
- (NSURLSessionWebSocketTask *)socketForStream:(NSString *)stream;
- (void)close;
@end
