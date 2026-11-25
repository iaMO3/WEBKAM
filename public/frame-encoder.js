// One canvas per worker. A slot accepts its next frame after this encode completes.
let surface;
let context;
self.onmessage = async ({data}) => {
  const {snapshot, geometry, quality, retryQuality, maxBytes} = data;
  try {
    const {outputWidth, outputHeight, rotation, width, height} = geometry;
    if (!surface || surface.width !== outputWidth || surface.height !== outputHeight) {
      surface = new OffscreenCanvas(outputWidth, outputHeight);
      context = surface.getContext('2d', { alpha: false });
    }
    context.setTransform(1, 0, 0, 1, outputWidth / 2, outputHeight / 2);
    context.rotate(rotation * Math.PI / 180);
    context.drawImage(snapshot, -width / 2, -height / 2, width, height);
    let jpeg = await surface.convertToBlob({type: 'image/jpeg', quality});
    if (jpeg.type !== 'image/jpeg') throw new Error('JPEG encoding unavailable');
    if (jpeg.size > maxBytes) jpeg = await surface.convertToBlob({type: 'image/jpeg', quality: retryQuality});
    self.postMessage({jpeg});
  } catch { self.postMessage({failed: true}); }
  finally { snapshot.close(); }
};
