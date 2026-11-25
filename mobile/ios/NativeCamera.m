#import "NativeCamera.h"
#import "PhoneLink.h"
#import "CameraInterface.h"
#import <AVFoundation/AVFoundation.h>
#import <AVKit/AVKit.h>
#import <CoreImage/CoreImage.h>
#import <ImageIO/ImageIO.h>

static const CGFloat jpegQuality = .88;
static const CGFloat largeFrameQuality = .80;
static const NSUInteger maxJPEGBytes = 1500000;

static BOOL changeTorch(AVCaptureDevice *device, BOOL on) {
    AVCaptureTorchMode mode = on ? AVCaptureTorchModeOn : AVCaptureTorchModeOff;
    if (!device.hasTorch || ![device isTorchModeSupported:mode] || (on && !device.isTorchAvailable)) return NO;
    if (device.torchMode == mode) return YES;
    if (![device lockForConfiguration:NULL]) return NO;
    BOOL success = YES;
    if (on) success = [device setTorchModeOnWithLevel:AVCaptureMaxAvailableTorchLevel error:NULL];
    else device.torchMode = AVCaptureTorchModeOff;
    [device unlockForConfiguration];
    return success;
}

@interface CameraPreview : UIView
@property (readonly) AVCaptureVideoPreviewLayer *previewLayer;
@end
@implementation CameraPreview
+ (Class)layerClass { return AVCaptureVideoPreviewLayer.class; }
- (AVCaptureVideoPreviewLayer *)previewLayer { return (AVCaptureVideoPreviewLayer *)self.layer; }
@end

@interface NativeCamera () <AVCaptureVideoDataOutputSampleBufferDelegate, AVPictureInPictureControllerDelegate> {
    dispatch_queue_t _captureQueue;
    AVCaptureSession *_capture;
    AVCaptureVideoDataOutput *_output;
    AVCaptureDeviceInput *_input;
    NSURLSessionWebSocketTask *_socket;
    CIContext *_imageContext, *_softwareContext;
    BOOL _backgroundEncoding, _screenLight;
    NSUInteger _sendsInFlight;
    AVCaptureVideoOrientation _orientation;
    CMTime _lastFrame;
    NSUInteger _captureEpoch, _width, _height, _fps, _rotation;
    BOOL _followPhone, _announced;
}
@property (strong) PhoneLink *link;
@property (strong) CameraInterface *cameraInterface;
@property (strong) CameraPreview *preview;
@property (copy) NSArray<AVCaptureDevice *> *lenses;
@property (copy) NSArray<NSDictionary *> *lensChoices;
@property (nonatomic, copy) NSString *statusText;
@property (copy) NSString *modeText;
@property (assign) BOOL portraitPreview;
@property (assign) BOOL cameraReady;
@property (assign) BOOL videoLive;
@property (assign) BOOL flashAvailable;
@property (assign) BOOL flashOn;
@property (assign) BOOL flashBusy;
@property (copy) NSString *flashNote;
@property (assign) BOOL checking;
@property (assign) BOOL retry;
@property (assign) BOOL pairing;
@property (strong) AVCaptureDevice *lens;
@property (strong) AVPictureInPictureController *pip;
@property (strong) CameraPreview *floatingPreview;
@property (copy) NSString *stream;
@property (assign) NSUInteger epoch;
@property (assign) BOOL paired;
@property (assign) BOOL opening;
@property (assign) BOOL pipStarting;
@property (assign) BOOL pipAudioActive;
@property (assign) BOOL observingOrientation;
@end

@implementation NativeCamera
- (instancetype)initWithHost:(NSString *)host {
    if ((self = [super initWithNibName:nil bundle:nil])) {
        self.link = [[PhoneLink alloc] initWithHost:host];
        _captureQueue = dispatch_queue_create("app.pocketcam.capture", DISPATCH_QUEUE_SERIAL);
        _capture = [AVCaptureSession new];
        _orientation = AVCaptureVideoOrientationPortrait;
        // The app can be in the background during PiP, where iOS restricts GPU work.
        _imageContext = [CIContext contextWithOptions:@{kCIContextCacheIntermediates:@NO}];
        _softwareContext = [CIContext contextWithOptions:@{
            kCIContextCacheIntermediates:@NO, kCIContextUseSoftwareRenderer:@YES
        }];
    }
    return self;
}

- (void)viewDidLoad {
    [super viewDidLoad];
    self.title = @"WEBKAM";
    self.view.backgroundColor = UIColor.systemBackgroundColor;
    self.preview = [CameraPreview new];
    self.preview.backgroundColor = [UIColor colorWithRed:.06 green:.08 blue:.14 alpha:1];
    self.preview.layer.cornerRadius = 20;
    self.preview.clipsToBounds = YES;
    self.preview.previewLayer.videoGravity = AVLayerVideoGravityResizeAspect;
    self.preview.previewLayer.session = _capture;
    __weak typeof(self) weakSelf = self;
    self.cameraInterface = [[CameraInterface alloc] initWithPreview:self.preview action:^(NSString *action, NSDictionary *payload) {
        [weakSelf cameraAction:action payload:payload];
    }];
    self.cameraInterface.translatesAutoresizingMaskIntoConstraints = NO;
    [self.view addSubview:self.cameraInterface];
    UILayoutGuide *safe = self.view.safeAreaLayoutGuide;
    [NSLayoutConstraint activateConstraints:@[
        [self.cameraInterface.topAnchor constraintEqualToAnchor:safe.topAnchor],
        [self.cameraInterface.bottomAnchor constraintEqualToAnchor:safe.bottomAnchor],
        [self.cameraInterface.leadingAnchor constraintEqualToAnchor:safe.leadingAnchor],
        [self.cameraInterface.trailingAnchor constraintEqualToAnchor:safe.trailingAnchor]
    ]];
    [self findLenses];
    [self checkPairing];

    NSNotificationCenter *notifications = NSNotificationCenter.defaultCenter;
    [UIDevice.currentDevice beginGeneratingDeviceOrientationNotifications];
    self.observingOrientation = YES;
    [notifications addObserver:self selector:@selector(orientationChanged:) name:UIDeviceOrientationDidChangeNotification object:nil];
    [notifications addObserver:self selector:@selector(renderStateChanged:) name:UIApplicationWillResignActiveNotification object:nil];
    [notifications addObserver:self selector:@selector(renderStateChanged:) name:UIApplicationDidBecomeActiveNotification object:nil];
    [notifications addObserver:self selector:@selector(backgroundNotification:) name:UIApplicationDidEnterBackgroundNotification object:nil];
    [notifications addObserver:self selector:@selector(captureInterrupted:) name:AVCaptureSessionWasInterruptedNotification object:_capture];
    [notifications addObserver:self selector:@selector(captureResumed:) name:AVCaptureSessionInterruptionEndedNotification object:_capture];
    [self orientationChanged:nil];
}

- (void)findLenses {
    NSArray *types = @[AVCaptureDeviceTypeBuiltInWideAngleCamera, AVCaptureDeviceTypeBuiltInUltraWideCamera,
        AVCaptureDeviceTypeBuiltInTelephotoCamera, AVCaptureDeviceTypeBuiltInTrueDepthCamera];
    self.lenses = [AVCaptureDeviceDiscoverySession
        discoverySessionWithDeviceTypes:types mediaType:AVMediaTypeVideo position:AVCaptureDevicePositionUnspecified].devices;
    self.lens = [AVCaptureDevice defaultDeviceWithDeviceType:AVCaptureDeviceTypeBuiltInWideAngleCamera
        mediaType:AVMediaTypeVideo position:AVCaptureDevicePositionBack] ?: self.lenses.firstObject;
    NSMutableArray<NSDictionary *> *choices = [NSMutableArray array];
    for (AVCaptureDevice *lens in self.lenses) {
        [choices addObject:@{@"name":lens.localizedName, @"id":lens.uniqueID}];
    }
    self.lensChoices = choices;
}

- (void)cameraAction:(NSString *)action payload:(NSDictionary *)payload {
    if ([action isEqualToString:@"pair"] && [payload[@"code"] isKindOfClass:NSString.class]) {
        [self pairPhone:payload[@"code"]];
    } else if ([action isEqualToString:@"retry"]) {
        [self checkPairing];
    } else if ([action isEqualToString:@"lens"] && self.paired
               && !self.opening && (!self.stream || self.cameraReady) && !self.pipStarting && !self.flashBusy) {
        for (AVCaptureDevice *lens in self.lenses) {
            if ([lens.uniqueID isEqual:payload[@"id"]]) {
                [self selectLens:lens];
                break;
            }
        }
    } else if ([action isEqualToString:@"camera"] && self.paired && self.lens) {
        [self toggleCamera];
    } else if ([action isEqualToString:@"flash"] && self.stream && self.cameraReady) {
        [self toggleFlash];
    } else if ([action isEqualToString:@"pip"] && self.stream && self.cameraReady) {
        [self togglePiP];
    } else if ([action isEqualToString:@"interface-reload"] && !self.pip.pictureInPictureActive) {
        [self stopWithMessage:@"Camera paused. Tap Start camera to reconnect."];
    }
}

- (void)selectLens:(AVCaptureDevice *)lens {
    if (self.lens == lens) return;
    self.lens = lens;
    self.flashAvailable = NO;
    self.flashOn = NO;
    self.flashNote = nil;
    [self updateControls];
    if (!self.stream) return;
    dispatch_async(_captureQueue, ^{
        if (![self installInput:lens]) {
            [self failCapture:@"The selected lens could not open. Try another lens."];
        } else {
            self->_lastFrame = kCMTimeInvalid;
            [self applyCaptureRate];
            [self publishFlashState:NO];
        }
    });
}

- (void)updateControls {
    BOOL running = self.stream || self.opening;
    NSString *phase = self.videoLive ? @"live" : self.retry ? @"offline"
        : (running || self.checking || self.pairing) ? @"pending" : self.paired ? @"ready" : @"pairing";
    [self.cameraInterface render:@{
        @"phase":phase, @"message":self.statusText ?: @"Connecting to your Windows PC…",
        @"paired":@(self.paired), @"checking":@(self.checking), @"retry":@(self.retry), @"pairing":@(self.pairing),
        @"running":@(running), @"opening":@(!self.cameraReady && running),
        @"preview":@(self.stream && self.cameraReady), @"portrait":@(self.portraitPreview),
        @"lenses":self.lensChoices ?: @[], @"lens":self.lens.uniqueID ?: @"",
        @"pip":@(self.stream && self.cameraReady && self.pip != nil), @"floating":@(self.pip.pictureInPictureActive),
        @"pipStarting":@(self.pipStarting), @"mode":self.modeText ?: @"",
        @"flashAvailable":@(self.stream && self.cameraReady && self.flashAvailable),
        @"screenLight":@(self.lens.position == AVCaptureDevicePositionFront),
        @"flashOn":@(self.flashOn), @"flashBusy":@(self.flashBusy),
        @"flashNote":self.flashNote ?: @""
    }];
}

- (void)toggleFlash {
    if (!self.flashAvailable || self.flashBusy) return;
    BOOL enable = !self.flashOn;
    self.flashBusy = YES;
    self.flashNote = nil;
    [self updateControls];
    dispatch_async(_captureQueue, ^{
        [self setCaptureLight:enable];
    });
}

- (void)setCaptureLight:(BOOL)enable {
    BOOL front = _input.device.position == AVCaptureDevicePositionFront;
    if (front) _screenLight = enable;
    [self publishFlashState:!(front || changeTorch(_input.device, enable))];
}

// Read the active camera on the capture queue. Publish only for the current lens and stream.
- (void)publishFlashState:(BOOL)failed {
    AVCaptureDevice *device = _input.device;
    BOOL front = device.position == AVCaptureDevicePositionFront;
    BOOL available = device.hasTorch && [device isTorchModeSupported:AVCaptureTorchModeOn];
    BOOL on = front ? _screenLight : available && device.torchMode == AVCaptureTorchModeOn;
    NSDictionary *state = @{@"light": front ? @"screen" : available ? @"torch" : @"none", @"on":@(on)};
    NSData *json = [NSJSONSerialization dataWithJSONObject:state options:0 error:NULL];
    NSString *text = [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding];
    if (_socket && text) [_socket sendMessage:[[NSURLSessionWebSocketMessage alloc] initWithString:text]
                            completionHandler:^(NSError *error) {}];
    NSUInteger epoch = _captureEpoch;
    dispatch_async(dispatch_get_main_queue(), ^{
        if (self.epoch != epoch || self.lens != device) return;
        self.flashAvailable = front || available;
        self.flashOn = on;
        self.flashBusy = NO;
        self.flashNote = failed ? @"The flash couldn’t switch. It may be temporarily unavailable." : nil;
        [self updateControls];
    });
}

- (void)setStatusText:(NSString *)statusText {
    _statusText = [statusText copy];
    [self updateControls];
}

- (void)checkPairing {
    if (self.checking || self.pairing || self.stream || self.opening) return;
    self.checking = YES;
    self.retry = NO;
    self.statusText = @"Connecting to your Windows PC…";
    __weak typeof(self) weakSelf = self;
    [self.link request:@"/api/phone" body:nil stream:nil completion:^(NSInteger code, NSDictionary *json, NSError *error) {
        NativeCamera *self = weakSelf;
        if (!self) return;
        self.checking = NO;
        self.paired = code == 200;
        self.retry = error != nil || (code != 200 && code != 401);
        self.statusText = error ? [self connectionMessage:error] : self.paired
            ? @"Paired with your Windows PC." : @"Enter the pairing code shown in WEBKAM on Windows.";
    }];
}

- (NSString *)connectionMessage:(NSError *)error {
    switch (error.code) {
        case NSURLErrorServerCertificateUntrusted:
        case NSURLErrorServerCertificateHasUnknownRoot:
        case NSURLErrorServerCertificateHasBadDate:
        case NSURLErrorSecureConnectionFailed:
            return @"Enable full trust for the certificate shown on your PC in iPhone Certificate Trust Settings, then tap Retry connection.";
        default:
            return @"Couldn’t reach your PC. Keep WEBKAM open on Windows and allow Local Network access on iPhone.";
    }
}

- (void)pairPhone:(NSString *)enteredCode {
    if (self.paired || self.checking || self.pairing || self.retry) return;
    NSString *code = [enteredCode stringByReplacingOccurrencesOfString:@" " withString:@""];
    NSCharacterSet *digits = [NSCharacterSet characterSetWithCharactersInString:@"0123456789"];
    if (code.length != 8 || [code rangeOfCharacterFromSet:digits.invertedSet].location != NSNotFound) {
        self.statusText = @"Enter the eight digits shown on Windows.";
        return;
    }
    self.pairing = YES;
    self.statusText = @"Pairing with your Windows PC…";
    __weak typeof(self) weakSelf = self;
    [self.link request:@"/api/pair" body:@{@"code":code} stream:nil completion:^(NSInteger status, NSDictionary *json, NSError *error) {
        NativeCamera *self = weakSelf;
        if (!self) return;
        self.pairing = NO;
        if (status == 204) {
            self.paired = YES;
            self.statusText = @"Paired with your Windows PC.";
        } else if (error) {
            self.retry = YES;
            self.statusText = [self connectionMessage:error];
        } else if (status == 409) {
            self.statusText = @"Select Disconnect in WEBKAM on Windows to get a new pairing code.";
        } else if (status == 429) {
            self.statusText = @"Wait 30 seconds before trying again.";
        } else {
            self.statusText = @"Check the pairing code on Windows and try again.";
        }
    }];
}

- (void)toggleCamera {
    if (self.stream || self.opening) {
        [self stopWithMessage:@"Camera is off."];
        return;
    }
    self.opening = YES;
    NSUInteger epoch = ++self.epoch;
    self.statusText = @"Starting camera…";
    __weak typeof(self) weakSelf = self;
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeVideo completionHandler:^(BOOL allowed) {
        dispatch_async(dispatch_get_main_queue(), ^{
            NativeCamera *self = weakSelf;
            if (!self || self.epoch != epoch) return;
            if (allowed) [self startStream:epoch];
            else [self stopWithMessage:@"Allow camera access for WEBKAM in iPhone Settings."];
        });
    }];
}

- (void)startStream:(NSUInteger)epoch {
    PhoneLink *link = self.link;
    __weak typeof(self) weakSelf = self;
    [link request:@"/api/start" body:@{} stream:nil completion:^(NSInteger code, NSDictionary *json, NSError *error) {
        NativeCamera *self = weakSelf;
        NSString *stream = [json[@"stream"] isKindOfClass:NSString.class] ? json[@"stream"] : nil;
        if (!self || self.epoch != epoch) {
            if (stream) [link request:@"/api/stop" body:nil stream:stream completion:nil];
            return;
        }
        if (code != 200 || !stream) {
            if (code == 401) self.paired = NO;
            if (error) {
                self.paired = NO;
                self.retry = YES;
            }
            [self stopWithMessage:error ? [self connectionMessage:error] : @"Pair with WEBKAM on Windows again."];
            return;
        }
        self.stream = stream;
        self.opening = NO;
        [self updateControls];
        AVCaptureDevice *lens = self.lens;
        dispatch_async(self->_captureQueue, ^{
            self->_captureEpoch = epoch;
            self->_socket = [link socketForStream:stream];
            self->_fps = 0;
            self->_lastFrame = kCMTimeInvalid;
            self->_sendsInFlight = 0;
            self->_screenLight = self->_announced = NO;
            [self->_socket resume];
            [self receiveMode:self->_socket lens:lens];
        });
    }];
}

- (void)receiveMode:(NSURLSessionWebSocketTask *)socket lens:(AVCaptureDevice *)lens {
    __weak typeof(self) weakSelf = self;
    [socket receiveMessageWithCompletionHandler:^(NSURLSessionWebSocketMessage *message, NSError *error) {
        NativeCamera *self = weakSelf;
        if (!self) return;
        dispatch_async(self->_captureQueue, ^{
            if (socket != self->_socket) return;
            if (error) {
                [self failCapture:@"Connection ended. Start the camera again."];
                return;
            }
            NSData *data = [message.string dataUsingEncoding:NSUTF8StringEncoding];
            id mode = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL] : nil;
            if ([mode isKindOfClass:NSDictionary.class] && [mode[@"setLight"] isKindOfClass:NSNumber.class]) {
                [self setCaptureLight:[mode[@"setLight"] boolValue]];
                [self receiveMode:socket lens:lens];
                return;
            }
            if (![mode isKindOfClass:NSDictionary.class] || ![self applyMode:mode lens:lens]) {
                [self failCapture:@"Couldn’t open the selected camera or video mode."];
                return;
            }
            [self receiveMode:socket lens:lens];
        });
    }];
}

- (BOOL)applyMode:(NSDictionary *)mode lens:(AVCaptureDevice *)lens {
    for (NSString *key in @[@"width",@"height",@"fps",@"rotation",@"follow_phone"]) {
        if (![mode[key] isKindOfClass:NSNumber.class]) return NO;
    }
    NSUInteger w = [mode[@"width"] unsignedIntegerValue], h = [mode[@"height"] unsignedIntegerValue];
    NSUInteger fps = [mode[@"fps"] unsignedIntegerValue], rotation = [mode[@"rotation"] unsignedIntegerValue];
    NSUInteger longEdge = MAX(w,h), shortEdge = MIN(w,h);
    if (!((longEdge == 1920 && shortEdge == 1080) || (longEdge == 1280 && shortEdge == 720))
        || (fps != 30 && fps != 60) || rotation > 270 || rotation % 90 != 0) return NO;
    BOOL firstMode = _fps == 0;
    _width = w;
    _height = h;
    _fps = fps;
    _rotation = rotation;
    _followPhone = [mode[@"follow_phone"] boolValue];
    if (firstMode && ![self startCapture:lens]) return NO;
    [self applyCaptureRate];
    [self publishMode];
    return YES;
}

- (BOOL)installInput:(AVCaptureDevice *)lens {
    AVCaptureDeviceInput *replacement = lens ? [AVCaptureDeviceInput deviceInputWithDevice:lens error:NULL] : nil;
    if (!replacement) return NO;
    [_capture beginConfiguration];
    _screenLight = NO;
    if (_input) {
        if (_input.device.torchMode == AVCaptureTorchModeOn) changeTorch(_input.device, NO);
        [_capture removeInput:_input];
    }
    BOOL accepted = [_capture canAddInput:replacement];
    if (accepted) {
        [_capture addInput:replacement];
        _input = replacement;
    } else if (_input && [_capture canAddInput:_input]) {
        [_capture addInput:_input];
    }
    [_capture commitConfiguration];
    [self updateOrientation];
    return accepted;
}

- (BOOL)startCapture:(AVCaptureDevice *)lens {
    if (![self installInput:lens]) return NO;
    [_capture beginConfiguration];
    if ([_capture canSetSessionPreset:AVCaptureSessionPreset1920x1080]) {
        _capture.sessionPreset = AVCaptureSessionPreset1920x1080;
    }
    if (_capture.multitaskingCameraAccessSupported) _capture.multitaskingCameraAccessEnabled = YES;
    if (!_output) {
        AVCaptureVideoDataOutput *output = [AVCaptureVideoDataOutput new];
        output.alwaysDiscardsLateVideoFrames = YES;
        [output setSampleBufferDelegate:self queue:_captureQueue];
        if ([_capture canAddOutput:output]) {
            [_capture addOutput:output];
            _output = output;
        }
    }
    [_capture commitConfiguration];
    if (!_output) return NO;
    [self updateOrientation];
    [_capture startRunning];
    if (!_capture.isRunning) return NO;
    [self publishFlashState:NO];
    BOOL floating = _capture.multitaskingCameraAccessEnabled;
    NSUInteger epoch = _captureEpoch;
    dispatch_async(dispatch_get_main_queue(), ^{
        if (self.epoch != epoch) return;
        self.cameraReady = YES;
        if (floating && [AVPictureInPictureController isPictureInPictureSupported]) {
            [self preparePiP];
        } else {
            self.statusText = @"Camera is open. This iPhone hasn’t enabled camera multitasking.";
        }
    });
    return YES;
}

- (void)applyCaptureRate {
    AVCaptureDevice *device = _input.device;
    if (!device) return;
    // A lens can expose 60 FPS on a different format than the 1080p preset chose.
    AVCaptureDeviceFormat *best = nil;
    NSInteger bestScore = NSIntegerMax;
    for (AVCaptureDeviceFormat *format in device.formats) {
        CMVideoDimensions size = CMVideoFormatDescriptionGetDimensions(format.formatDescription);
        if (size.width < (int)MAX(_width,_height) || size.height < (int)MIN(_width,_height)) continue;
        for (AVFrameRateRange *range in format.videoSupportedFrameRateRanges) {
            if (_fps < range.minFrameRate || _fps > range.maxFrameRate) continue;
            NSInteger score = (NSInteger)size.width * size.height * 2 + (format != device.activeFormat);
            if (score < bestScore) { best = format; bestScore = score; }
            break;
        }
    }
    if (!best) return;
    [_capture beginConfiguration];
    if ([device lockForConfiguration:NULL]) {
        if (device.activeFormat != best) device.activeFormat = best;
        CMTime duration = CMTimeMake(1, (int32_t)_fps);
        device.activeVideoMinFrameDuration = duration;
        device.activeVideoMaxFrameDuration = duration;
        [device unlockForConfiguration];
    }
    [_capture commitConfiguration];
    [self updateOrientation];
}

- (void)orientationChanged:(NSNotification *)notification {
    AVCaptureVideoOrientation orientation;
    switch (UIDevice.currentDevice.orientation) {
        case UIDeviceOrientationLandscapeLeft: orientation = AVCaptureVideoOrientationLandscapeRight; break;
        case UIDeviceOrientationLandscapeRight: orientation = AVCaptureVideoOrientationLandscapeLeft; break;
        case UIDeviceOrientationPortrait: orientation = AVCaptureVideoOrientationPortrait; break;
        case UIDeviceOrientationPortraitUpsideDown: orientation = AVCaptureVideoOrientationPortraitUpsideDown; break;
        default: return;
    }
    dispatch_async(_captureQueue, ^{
        self->_orientation = orientation;
        [self updateOrientation];
        if (self->_fps > 0) [self publishMode];
    });
}

- (void)publishMode {
    BOOL devicePortrait = _orientation == AVCaptureVideoOrientationPortrait
        || _orientation == AVCaptureVideoOrientationPortraitUpsideDown;
    BOOL portrait = _followPhone ? devicePortrait != (_rotation % 180 != 0) : _height > _width;
    NSString *label = [NSString stringWithFormat:@"%lup · %lu FPS target",
        (unsigned long)MIN(_width,_height), (unsigned long)_fps];
    NSUInteger epoch = _captureEpoch;
    dispatch_async(dispatch_get_main_queue(), ^{
        if (self.epoch != epoch) return;
        self.portraitPreview = portrait;
        self.modeText = label;
        self.pip.contentSource.activeVideoCallContentViewController.preferredContentSize =
            portrait ? CGSizeMake(270,480) : CGSizeMake(480,270);
        [self updateControls];
    });
}

- (void)updateOrientation {
    AVCaptureConnection *connection = [_output connectionWithMediaType:AVMediaTypeVideo];
    if (connection.isVideoOrientationSupported) connection.videoOrientation = _orientation;
    AVCaptureVideoOrientation orientation = _orientation;
    dispatch_async(dispatch_get_main_queue(), ^{
        AVCaptureConnection *preview = self.preview.previewLayer.connection;
        if (preview.isVideoOrientationSupported) preview.videoOrientation = orientation;
        AVCaptureConnection *overlay = self.floatingPreview.previewLayer.connection;
        if (overlay.isVideoOrientationSupported) overlay.videoOrientation = orientation;
    });
}

- (NSData *)encode:(CVPixelBufferRef)buffer quality:(CGFloat)quality {
    if (!buffer) return nil;
    CIImage *image = [CIImage imageWithCVPixelBuffer:buffer];
    CGImagePropertyOrientation rotations[] = {
        kCGImagePropertyOrientationUp, kCGImagePropertyOrientationRight,
        kCGImagePropertyOrientationDown, kCGImagePropertyOrientationLeft
    };
    image = [image imageByApplyingCGOrientation:rotations[_rotation/90]];
    CGSize source = image.extent.size;
    if (source.width <= 0 || source.height <= 0) return nil;
    CGFloat w = _width, h = _height;
    if (_followPhone && (source.width > source.height) != (w > h)) {
        CGFloat previous = w;
        w = h;
        h = previous;
    }
    CGFloat scale = MAX(w/source.width,h/source.height);
    image = [image imageByApplyingTransform:CGAffineTransformMakeScale(scale,scale)];
    CGRect bounds = image.extent;
    CGRect crop = CGRectMake(CGRectGetMidX(bounds)-w/2, CGRectGetMidY(bounds)-h/2, w, h);
    image = [[image imageByCroppingToRect:crop]
        imageByApplyingTransform:CGAffineTransformMakeTranslation(-crop.origin.x,-crop.origin.y)];
    CGColorSpaceRef color = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CIContext *encoder = _backgroundEncoding ? _softwareContext : _imageContext;
    NSData *jpeg = [encoder JPEGRepresentationOfImage:image colorSpace:color
        options:@{(__bridge NSString *)kCGImageDestinationLossyCompressionQuality:@(quality)}];
    if (!jpeg && !_backgroundEncoding)
        jpeg = [_softwareContext JPEGRepresentationOfImage:image colorSpace:color
            options:@{(__bridge NSString *)kCGImageDestinationLossyCompressionQuality:@(quality)}];
    CGColorSpaceRelease(color);
    return jpeg;
}

- (void)captureOutput:(AVCaptureOutput *)output didOutputSampleBuffer:(CMSampleBufferRef)sample
    fromConnection:(AVCaptureConnection *)connection {
    NSURLSessionWebSocketTask *socket = _socket;
    if (!socket || !_fps || _sendsInFlight >= 2) return;
    CMTime pts = CMSampleBufferGetPresentationTimeStamp(sample);
    if (CMTIME_IS_VALID(_lastFrame) && CMTimeGetSeconds(CMTimeSubtract(pts,_lastFrame)) < .9/_fps) return;
    _lastFrame = pts;
    @autoreleasepool {
        CVImageBufferRef image = CMSampleBufferGetImageBuffer(sample);
        NSData *jpeg = [self encode:image quality:jpegQuality];
        if (jpeg.length > maxJPEGBytes) jpeg = [self encode:image quality:largeFrameQuality];
        if (!jpeg.length || jpeg.length > maxJPEGBytes) return;
        _sendsInFlight++;
        __weak typeof(self) weakSelf = self;
        [socket sendMessage:[[NSURLSessionWebSocketMessage alloc] initWithData:jpeg] completionHandler:^(NSError *error) {
            NativeCamera *self = weakSelf;
            if (!self) return;
            dispatch_async(self->_captureQueue, ^{
                if (socket != self->_socket) return;
                self->_sendsInFlight--;
                if (error) {
                    [self failCapture:@"Connection ended. Start the camera again."];
                } else if (!self->_announced) {
                    self->_announced = YES;
                    NSUInteger epoch = self->_captureEpoch;
                    dispatch_async(dispatch_get_main_queue(), ^{
                        if (self.epoch != epoch) return;
                        self.videoLive = YES;
                        self.statusText = self.pip ? @"Live on your Windows PC."
                            : @"Live on Windows. This iPhone hasn’t enabled camera multitasking.";
                    });
                }
            });
        }];
    }
}

- (void)failCapture:(NSString *)message {
    NSUInteger epoch = _captureEpoch;
    dispatch_async(dispatch_get_main_queue(), ^{ if (self.epoch == epoch) [self stopWithMessage:message]; });
}

- (void)preparePiP {
    [AVAudioSession.sharedInstance setCategory:AVAudioSessionCategoryPlayback
        withOptions:AVAudioSessionCategoryOptionMixWithOthers error:NULL];
    AVPictureInPictureVideoCallViewController *call = [AVPictureInPictureVideoCallViewController new];
    call.preferredContentSize = CGSizeMake(480,270);
    CameraPreview *floating = [[CameraPreview alloc] initWithFrame:call.view.bounds];
    floating.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
    floating.backgroundColor = UIColor.blackColor;
    floating.previewLayer.videoGravity = AVLayerVideoGravityResizeAspect;
    floating.previewLayer.session = _capture;
    self.floatingPreview = floating;
    [call.view addSubview:floating];
    AVPictureInPictureControllerContentSource *source = [[AVPictureInPictureControllerContentSource alloc]
        initWithActiveVideoCallSourceView:self.preview contentViewController:call];
    self.pip = [[AVPictureInPictureController alloc] initWithContentSource:source];
    self.pip.delegate = self;
    self.pip.canStartPictureInPictureAutomaticallyFromInline = NO;
    dispatch_async(_captureQueue, ^{ [self updateOrientation]; });
    [self updateControls];
}

- (void)togglePiP {
    if (!self.pip || self.pipStarting) return;
    if (self.pip.pictureInPictureActive) {
        [self.pip stopPictureInPicture];
        return;
    }
    AVAudioSession *audio = AVAudioSession.sharedInstance;
    self.pipAudioActive = [audio setActive:YES error:NULL];
    self.pipStarting = YES;
    [self updateControls];
    [self startPiPWhenPossible:self.pip deadline:CACurrentMediaTime() + 2];
}

- (void)startPiPWhenPossible:(AVPictureInPictureController *)controller deadline:(CFTimeInterval)deadline {
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(0.2 * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{
        if (self.pip != controller || !self.pipStarting) return;
        if (controller.pictureInPicturePossible) {
            [controller startPictureInPicture];
        } else if (CACurrentMediaTime() < deadline) {
            [self startPiPWhenPossible:controller deadline:deadline];
        } else {
            self.pipStarting = NO;
            [self releasePiPAudio];
            self.statusText = @"Floating preview isn’t available right now. Check Picture in Picture in iPhone Settings.";
            [self backgrounded];
        }
    });
}

- (void)releasePiPAudio {
    if (!self.pipAudioActive) return;
    self.pipAudioActive = NO;
    [AVAudioSession.sharedInstance setActive:NO error:NULL];
}

- (void)pictureInPictureControllerDidStartPictureInPicture:(AVPictureInPictureController *)controller {
    if (controller != self.pip) return;
    self.pipStarting = NO;
    [self updateControls];
}

- (void)pictureInPictureController:(AVPictureInPictureController *)controller
    failedToStartPictureInPictureWithError:(NSError *)error {
    if (controller != self.pip) return;
    self.pipStarting = NO;
    [self releasePiPAudio];
    self.statusText = @"iOS couldn’t start floating preview. Keep WEBKAM open to continue streaming.";
    [self backgrounded];
}

- (void)pictureInPictureControllerDidStopPictureInPicture:(AVPictureInPictureController *)controller {
    if (controller != self.pip) return;
    self.pipStarting = NO;
    [self releasePiPAudio];
    [self updateControls];
    [self backgrounded];
}

- (void)pictureInPictureController:(AVPictureInPictureController *)controller
    restoreUserInterfaceForPictureInPictureStopWithCompletionHandler:(void (^)(BOOL))completion {
    completion(self.navigationController.topViewController == self);
}

- (void)backgrounded {
    if (UIApplication.sharedApplication.applicationState == UIApplicationStateBackground
        && (self.stream || self.opening) && !self.pip.pictureInPictureActive && !self.pipStarting) {
        [self stopWithMessage:@"Camera paused. Tap Start camera to reconnect."];
    }
}

- (void)renderStateChanged:(NSNotification *)notification {
    BOOL background = ![notification.name isEqual:UIApplicationDidBecomeActiveNotification];
    dispatch_async(_captureQueue, ^{ self->_backgroundEncoding = background; });
}

- (void)backgroundNotification:(NSNotification *)notification {
    [self backgrounded];
}

- (void)captureInterrupted:(NSNotification *)notification {
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!self.stream) return;
        self.videoLive = NO;
        self.statusText = @"iPhone paused the camera. Return to WEBKAM to continue.";
    });
}

- (void)captureResumed:(NSNotification *)notification {
    dispatch_async(_captureQueue, ^{
        self->_announced = NO;
        self->_lastFrame = kCMTimeInvalid;
        [self publishFlashState:NO];
    });
}

- (void)stopWithMessage:(NSString *)message {
    self.epoch++;
    self.opening = NO;
    NSString *stream = self.stream;
    self.stream = nil;
    self.cameraReady = NO;
    self.videoLive = NO;
    self.flashAvailable = NO;
    self.flashOn = NO;
    self.flashBusy = NO;
    self.flashNote = nil;
    self.portraitPreview = NO;
    self.modeText = nil;
    self.pipStarting = NO;
    self.pip.delegate = nil;
    [self.pip stopPictureInPicture];
    self.pip = nil;
    self.floatingPreview = nil;
    [self releasePiPAudio];
    if (stream) [self.link request:@"/api/stop" body:nil stream:stream completion:nil];
    dispatch_async(_captureQueue, ^{
        [self->_socket cancelWithCloseCode:NSURLSessionWebSocketCloseCodeNormalClosure reason:nil];
        self->_socket = nil;
        self->_fps = 0;
        if (self->_input.device.torchMode == AVCaptureTorchModeOn) changeTorch(self->_input.device, NO);
        [self->_capture stopRunning];
    });
    self.statusText = message;
}

- (void)viewWillDisappear:(BOOL)animated {
    [super viewWillDisappear:animated];
    if (self.isMovingFromParentViewController) [self stopWithMessage:@"Camera is off."];
}

- (void)viewDidDisappear:(BOOL)animated {
    [super viewDidDisappear:animated];
    if (!self.navigationController && self.observingOrientation) {
        [UIDevice.currentDevice endGeneratingDeviceOrientationNotifications];
        self.observingOrientation = NO;
    }
}

- (void)dealloc {
    [NSNotificationCenter.defaultCenter removeObserver:self];
    [self.link close];
}
@end
