#import <UIKit/UIKit.h>

typedef void (^CameraInterfaceAction)(NSString *action, NSDictionary *payload);

@interface CameraInterface : UIView
- (instancetype)initWithPreview:(UIView *)preview action:(CameraInterfaceAction)action;
- (void)render:(NSDictionary *)state;
@end
