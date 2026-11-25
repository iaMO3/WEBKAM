#import <CoreFoundation/CFUserNotification.h>
#import <objc/runtime.h>
#import <Foundation/NSData.h>
#import <Foundation/NSDictionary.h>
#import <Foundation/NSValue.h>
#import <Foundation/NSString.h>
#import <CoreMediaIO/CMIOHardware.h>
#import <CoreMedia/CMSync.h>
#import <VideoToolbox/VTPixelTransferSession.h>
#import <VideoToolbox/VTPixelTransferProperties.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

// Published device UID from VirtualCamera4Mac's producer protocol (v0.0.7).
static NSString *const cameraUID = @"B9E1C0F2-6A2E-4B7D-9C3A-2F1E7D4A5C6B";
static CMIOObjectPropertyAddress property(UInt32 selector) {
    return (CMIOObjectPropertyAddress){selector, kCMIOObjectPropertyScopeGlobal, kCMIOObjectPropertyElementMain};
}

static NSData *objectList(CMIOObjectID owner, UInt32 selector) {
    CMIOObjectPropertyAddress address = property(selector);
    UInt32 size = 0, used = 0;
    if (CMIOObjectGetPropertyDataSize(owner, &address, 0, NULL, &size) || !size) return nil;
    NSMutableData *data = [NSMutableData dataWithLength:size];
    if (CMIOObjectGetPropertyData(owner, &address, 0, NULL, size, &used, data.mutableBytes)) return nil;
    data.length = used;
    return data;
}

uint32_t webkam_device(void) {
    @autoreleasepool {
        NSData *devices = objectList(kCMIOObjectSystemObject, kCMIOHardwarePropertyDevices);
        const CMIOObjectID *ids = devices.bytes;
        for (NSUInteger i = 0; i < devices.length / sizeof(*ids); i++) {
            CMIOObjectPropertyAddress key = property(kCMIODevicePropertyDeviceUID);
            CFStringRef uid = NULL;
            UInt32 used = 0;
            if (CMIOObjectGetPropertyData(ids[i], &key, 0, NULL, sizeof(uid), &used, &uid) || !uid) continue;
            BOOL match = [(__bridge NSString *)uid caseInsensitiveCompare:cameraUID] == NSOrderedSame;
            CFRelease(uid);
            if (match) return ids[i];
        }
        return 0;
    }
}

typedef struct {
    CMIOObjectID device, sink, source;
    CMSimpleQueueRef queue;
    VTPixelTransferSessionRef transfer;
    CVPixelBufferPoolRef pool;
    CMFormatDescriptionRef format, sampleFormat;
} Output;

static void queueChanged(CMIOStreamID stream, void *token, void *refcon) {}

void webkam_close(Output *out) {
    if (!out) return;
    if (out->queue) {
        CMIODeviceStopStream(out->device, out->sink);
        CFRelease(out->queue);
    }
    if (out->pool) CFRelease(out->pool);
    if (out->format) CFRelease(out->format);
    if (out->sampleFormat) CFRelease(out->sampleFormat);
    if (out->transfer) CFRelease(out->transfer);
    free(out);
}

Output *webkam_open(uint32_t device) {
    @autoreleasepool {
        NSData *streams = objectList(device, kCMIODevicePropertyStreams);
        if (streams.length != 2 * sizeof(CMIOStreamID)) return NULL;
        const CMIOStreamID *ids = streams.bytes;
        Output *out = calloc(1, sizeof(*out));
        if (!out) return NULL;
        // The published device has a source followed by the producer sink.
        out->device = device; out->source = ids[0]; out->sink = ids[1];
        if (VTPixelTransferSessionCreate(NULL, &out->transfer) ||
            VTSessionSetProperty(out->transfer, kVTPixelTransferPropertyKey_ScalingMode, kVTScalingMode_Letterbox) ||
            CMIOStreamCopyBufferQueue(out->sink, queueChanged, NULL, &out->queue) ||
            !out->queue || CMIODeviceStartStream(device, out->sink)) {
            webkam_close(out); return NULL;
        }
        return out;
    }
}

static bool prepareFormat(Output *out) {
    CMIOObjectPropertyAddress key = property(kCMIOStreamPropertyFormatDescription);
    CMFormatDescriptionRef fmt = NULL;
    UInt32 used = 0;
    if (CMIOObjectGetPropertyData(out->source, &key, 0, NULL, sizeof(fmt), &used, &fmt) || !fmt) return false;
    if (out->format && CMFormatDescriptionEqual(out->format, fmt)) { CFRelease(fmt); return true; }
    CMVideoDimensions dims = CMVideoFormatDescriptionGetDimensions(fmt);
    OSType pixel = CMFormatDescriptionGetMediaSubType(fmt);
    if (dims.width <= 0 || dims.width > 1920 || dims.height <= 0 || dims.height > 1080 ||
        (pixel != kCVPixelFormatType_32BGRA && pixel != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange)) {
        CFRelease(fmt); return false;
    }
    NSDictionary *attrs = @{(id)kCVPixelBufferWidthKey: @(dims.width), (id)kCVPixelBufferHeightKey: @(dims.height),
        (id)kCVPixelBufferPixelFormatTypeKey: @(pixel), (id)kCVPixelBufferIOSurfacePropertiesKey: @{}};
    CVPixelBufferPoolRef pool = NULL;
    if (CVPixelBufferPoolCreate(NULL, NULL, (__bridge CFDictionaryRef)attrs, &pool)) { CFRelease(fmt); return false; }
    if (out->format) CFRelease(out->format);
    if (out->pool) CFRelease(out->pool);
    if (out->sampleFormat) CFRelease(out->sampleFormat);
    out->sampleFormat = NULL;
    out->pool = pool; out->format = fmt;
    return true;
}

// One producer thread owns Output. A full queue drops the incoming frame.
bool webkam_send(Output *out, size_t width, size_t height, const uint8_t *nv12) {
    @autoreleasepool {
        if (CMSimpleQueueGetCount(out->queue) >= CMSimpleQueueGetCapacity(out->queue)) return true;
        if (!prepareFormat(out)) return false;
        CVPixelBufferRef src = NULL, dst = NULL;
        CMSampleBufferRef sample = NULL;
        bool ok = false;
        if (CVPixelBufferCreate(NULL, width, height, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, NULL, &src)) goto done;
        if (CVPixelBufferLockBaseAddress(src, 0)) goto done;
        for (size_t plane = 0; plane < 2; plane++) {
            uint8_t *ptr = CVPixelBufferGetBaseAddressOfPlane(src, plane);
            size_t stride = CVPixelBufferGetBytesPerRowOfPlane(src, plane);
            for (size_t y = 0; y < CVPixelBufferGetHeightOfPlane(src, plane); y++) {
                memcpy(ptr + y * stride, nv12, width);
                nv12 += width;
            }
        }
        CVPixelBufferUnlockBaseAddress(src, 0);
        CVBufferSetAttachment(src, kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_709_2, kCVAttachmentMode_ShouldPropagate);
        if (CVPixelBufferPoolCreatePixelBuffer(NULL, out->pool, &dst) ||
            VTPixelTransferSessionTransferImage(out->transfer, src, dst)) goto done;
        CMSampleTimingInfo timing = {CMTimeMake(1, 30), CMClockGetTime(CMClockGetHostTimeClock()), kCMTimeInvalid};
        if (!out->sampleFormat &&
            CMVideoFormatDescriptionCreateForImageBuffer(NULL, dst, &out->sampleFormat)) goto done;
        if (CMSampleBufferCreateReadyWithImageBuffer(NULL, dst, out->sampleFormat, &timing, &sample)) goto done;
        if (!CMSimpleQueueEnqueue(out->queue, sample)) { sample = NULL; ok = true; } // ownership passes to CMIO
    done:
        if (sample) CFRelease(sample);
        if (dst) CFRelease(dst);
        if (src) CFRelease(src);
        return ok;
    }
}

@interface NSObject (WebkamWorkspace)
+ (id)sharedWorkspace;
- (BOOL)accessibilityDisplayShouldReduceMotion;
@end

bool webkam_reduce_motion(void) {
    @autoreleasepool { return [[objc_getClass("NSWorkspace") sharedWorkspace] accessibilityDisplayShouldReduceMotion]; }
}

void webkam_error(const char *message) {
    @autoreleasepool {
        CFUserNotificationDisplayAlert(0, kCFUserNotificationStopAlertLevel, NULL, NULL, NULL,
            CFSTR("WEBKAM"), (__bridge CFStringRef)[NSString stringWithUTF8String:message],
            CFSTR("OK"), NULL, NULL, NULL);
    }
}
