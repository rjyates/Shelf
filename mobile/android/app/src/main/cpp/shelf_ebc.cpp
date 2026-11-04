// Experimental Nomad /dev/ebc HTEINK_IOC_DISPAREA path (Shelf-owned).
// Default ink stays Surface+SPEED+A2. Enable via ShelfEbc / debug.shelf.ink.ebc.
#include <android/log.h>
#include <fcntl.h>
#include <jni.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

#include <cerrno>
#include <cstdint>
#include <cstring>
#include <mutex>

#define LOG_TAG "ShelfEbc"
#define ALOGI(...) __android_log_print(ANDROID_LOG_INFO, LOG_TAG, __VA_ARGS__)
#define ALOGW(...) __android_log_print(ANDROID_LOG_WARN, LOG_TAG, __VA_ARGS__)

namespace {

constexpr uint32_t kIocGetinfo = 0x48545201u;
constexpr uint32_t kIocDisparea = 0x48545701u;

// Waveform byte used by libeinkutils postEinkHostBmpRectFast default / A2-ish.
constexpr uint8_t kModeDefault = 7;
constexpr uint8_t kModeA2ish = 4;

struct HteinkInfo {
  uint32_t hdr;
  uint16_t w;
  uint16_t h;
  uint32_t vir_w;
  uint32_t fbsize;
};

struct HteinkDispArea {
  int32_t left;
  int32_t top;
  int32_t right;
  int32_t bottom;
  int32_t offset;
  uint8_t mode;
  uint8_t pad[3];
};

std::mutex gMu;
int gFd = -1;
uint8_t* gMap = nullptr;
size_t gMapSize = 0;
int gWidth = 0;
int gHeight = 0;
int gStride = 0;  // pixels per row (vir_w)
uint8_t gMode = kModeA2ish;
bool gReady = false;

bool openEbcLocked() {
  if (gReady) return true;
  gFd = open("/dev/ebc", O_RDWR | O_CLOEXEC);
  if (gFd < 0) {
    ALOGW("open /dev/ebc failed errno=%d", errno);
    return false;
  }
  HteinkInfo info{};
  if (ioctl(gFd, kIocGetinfo, &info) != 0) {
    ALOGW("GETINFO failed errno=%d", errno);
    close(gFd);
    gFd = -1;
    return false;
  }
  gWidth = info.w > 0 ? info.w : static_cast<int>(info.vir_w);
  gHeight = info.h;
  gStride = info.vir_w > 0 ? static_cast<int>(info.vir_w) : gWidth;
  gMapSize = info.fbsize > 0 ? info.fbsize
                             : static_cast<size_t>(gStride) * static_cast<size_t>(gHeight);
  if (gWidth <= 0 || gHeight <= 0 || gMapSize == 0) {
    ALOGW("GETINFO bad geom w=%d h=%d fb=%zu", gWidth, gHeight, gMapSize);
    close(gFd);
    gFd = -1;
    return false;
  }
  void* map = mmap(nullptr, gMapSize, PROT_READ | PROT_WRITE, MAP_SHARED, gFd, 0);
  if (map == MAP_FAILED) {
    ALOGW("mmap failed errno=%d", errno);
    close(gFd);
    gFd = -1;
    return false;
  }
  gMap = static_cast<uint8_t*>(map);
  gReady = true;
  ALOGI("ebc ready %dx%d stride=%d fb=%zu mode=%u", gWidth, gHeight, gStride, gMapSize,
        gMode);
  return true;
}

void closeEbcLocked() {
  if (gMap != nullptr && gMapSize > 0) {
    munmap(gMap, gMapSize);
  }
  gMap = nullptr;
  gMapSize = 0;
  if (gFd >= 0) {
    close(gFd);
    gFd = -1;
  }
  gReady = false;
  gWidth = gHeight = gStride = 0;
}

}  // namespace

extern "C" JNIEXPORT jboolean JNICALL
Java_app_shelf_shelf_1mobile_ShelfEbc_nativeProbe(JNIEnv*, jclass) {
  std::lock_guard<std::mutex> lock(gMu);
  if (gReady) return JNI_TRUE;
  bool ok = openEbcLocked();
  // Probe should not keep the device open if caller only asked probe — but
  // keeping it warm avoids reopen cost for the experiment session.
  return ok ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT jboolean JNICALL
Java_app_shelf_shelf_1mobile_ShelfEbc_nativeBegin(JNIEnv*, jclass) {
  std::lock_guard<std::mutex> lock(gMu);
  return openEbcLocked() ? JNI_TRUE : JNI_FALSE;
}

extern "C" JNIEXPORT void JNICALL
Java_app_shelf_shelf_1mobile_ShelfEbc_nativeEnd(JNIEnv*, jclass) {
  std::lock_guard<std::mutex> lock(gMu);
  closeEbcLocked();
}

extern "C" JNIEXPORT void JNICALL
Java_app_shelf_shelf_1mobile_ShelfEbc_nativeSetMode(JNIEnv*, jclass, jint mode) {
  std::lock_guard<std::mutex> lock(gMu);
  if (mode == static_cast<jint>(kModeDefault) || mode == static_cast<jint>(kModeA2ish) ||
      (mode >= 0 && mode <= 15)) {
    gMode = static_cast<uint8_t>(mode);
  }
}

extern "C" JNIEXPORT jboolean JNICALL
Java_app_shelf_shelf_1mobile_ShelfEbc_nativePostRgba(
    JNIEnv* env,
    jclass,
    jintArray rgba,
    jint bmpW,
    jint bmpH,
    jint dirtyL,
    jint dirtyT,
    jint dirtyR,
    jint dirtyB,
    jint screenOriginX,
    jint screenOriginY) {
  if (rgba == nullptr || bmpW <= 0 || bmpH <= 0) return JNI_FALSE;

  std::lock_guard<std::mutex> lock(gMu);
  if (!gReady && !openEbcLocked()) return JNI_FALSE;

  int l = dirtyL;
  int t = dirtyT;
  int r = dirtyR;
  int b = dirtyB;
  if (l < 0) l = 0;
  if (t < 0) t = 0;
  if (r > bmpW) r = bmpW;
  if (b > bmpH) b = bmpH;
  if (r <= l || b <= t) return JNI_FALSE;

  const jsize len = env->GetArrayLength(rgba);
  if (len < bmpW * bmpH) return JNI_FALSE;
  jint* pixels = env->GetIntArrayElements(rgba, nullptr);
  if (pixels == nullptr) return JNI_FALSE;

  // ARGB_8888 ints → gray8 panel FB. Ink (opaque dark) → 0; leave other FB
  // pixels alone (RMW) so we do not blank the page.
  for (int y = t; y < b; ++y) {
    const int srcRow = y * bmpW;
    const int dstY = screenOriginY + y;
    if (dstY < 0 || dstY >= gHeight) continue;
    uint8_t* dstRow = gMap + static_cast<size_t>(dstY) * static_cast<size_t>(gStride);
    for (int x = l; x < r; ++x) {
      const int dstX = screenOriginX + x;
      if (dstX < 0 || dstX >= gWidth) continue;
      const uint32_t c = static_cast<uint32_t>(pixels[srcRow + x]);
      const uint32_t a = (c >> 24) & 0xffu;
      if (a < 32u) continue;  // transparent host pixel
      const uint32_t R = (c >> 16) & 0xffu;
      const uint32_t G = (c >> 8) & 0xffu;
      const uint32_t B = c & 0xffu;
      // Eraser clears to transparent in the host bmp; treat near-white opaque as
      // white (0xff) so we can scrub strokes on the panel FB.
      if (R > 200u && G > 200u && B > 200u) {
        dstRow[dstX] = 0xff;
      } else {
        dstRow[dstX] = 0x00;
      }
    }
  }
  env->ReleaseIntArrayElements(rgba, pixels, JNI_ABORT);

  HteinkDispArea area{};
  area.left = screenOriginX + l;
  area.top = screenOriginY + t;
  area.right = screenOriginX + r;
  area.bottom = screenOriginY + b;
  if (area.left < 0) area.left = 0;
  if (area.top < 0) area.top = 0;
  if (area.right > gWidth) area.right = gWidth;
  if (area.bottom > gHeight) area.bottom = gHeight;
  if (area.right <= area.left || area.bottom <= area.top) return JNI_FALSE;
  area.offset = 0;
  area.mode = gMode;

  const int rc = ioctl(gFd, kIocDisparea, &area);
  if (rc != 0) {
    ALOGW("DISPAREA failed ret=%d errno=%d [%d,%d-%d,%d] mode=%u", rc, errno, area.left,
          area.top, area.right, area.bottom, area.mode);
    return JNI_FALSE;
  }
  return JNI_TRUE;
}

extern "C" JNIEXPORT jstring JNICALL
Java_app_shelf_shelf_1mobile_ShelfEbc_nativeDiagnose(JNIEnv* env, jclass) {
  std::lock_guard<std::mutex> lock(gMu);
  char buf[192];
  snprintf(buf, sizeof buf, "ready=%d fd=%d %dx%d stride=%d fb=%zu mode=%u", gReady ? 1 : 0,
           gFd, gWidth, gHeight, gStride, gMapSize, gMode);
  return env->NewStringUTF(buf);
}
