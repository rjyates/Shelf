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

// GETINFO writes more than the documented head fields (probe: ≥0x18 bytes;
// libeinkutils keeps a larger blob at ebc+0x3c). Undersized stack buffer
// → stack-protector abort on Nomad.
struct HteinkInfo {
  uint32_t hdr;
  uint16_t w;
  uint16_t h;
  uint32_t vir_w;
  uint32_t fbsize;
  uint8_t rest[240];
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
  // Lightweight: do not mmap on the UI thread during plugin diagnose.
  if (gReady) return JNI_TRUE;
  const int fd = open("/dev/ebc", O_RDWR | O_CLOEXEC);
  if (fd < 0) return JNI_FALSE;
  HteinkInfo info{};
  const int rc = ioctl(fd, kIocGetinfo, &info);
  close(fd);
  return (rc == 0 && info.w > 0 && info.h > 0) ? JNI_TRUE : JNI_FALSE;
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

// Nomad panel FB is landscape (1872×1404) while the app UI is portrait
// (1404×1872). Map portrait screen → panel with 90° CW:
//   px = sy;  py = (panelH-1) - sx
static void screenToPanel(
    int sx,
    int sy,
    int screenW,
    int screenH,
    int* px,
    int* py) {
  if (gWidth > gHeight && screenH > screenW) {
    *px = sy;
    *py = (gHeight - 1) - sx;
    return;
  }
  if (gHeight > gWidth && screenW > screenH) {
    // Inverse case (app landscape, panel portrait) — 90° CCW.
    *px = (gWidth - 1) - sy;
    *py = sx;
    return;
  }
  *px = sx;
  *py = sy;
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
    jint screenOriginY,
    jint screenW,
    jint screenH) {
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

  int outL = gWidth;
  int outT = gHeight;
  int outR = 0;
  int outB = 0;

  // ARGB_8888 → gray8 panel FB. Any coverage (incl. AA fringe) stamps ink so
  // re-posted stroke rects stay continuous under A2-ish waveforms.
  for (int y = t; y < b; ++y) {
    const int srcRow = y * bmpW;
    for (int x = l; x < r; ++x) {
      const uint32_t c = static_cast<uint32_t>(pixels[srcRow + x]);
      const uint32_t a = (c >> 24) & 0xffu;
      if (a == 0u) continue;
      const int sx = screenOriginX + x;
      const int sy = screenOriginY + y;
      int px = 0;
      int py = 0;
      screenToPanel(sx, sy, screenW, screenH, &px, &py);
      if (px < 0 || py < 0 || px >= gWidth || py >= gHeight) continue;
      uint8_t* dst = gMap + static_cast<size_t>(py) * static_cast<size_t>(gStride) + px;
      const uint32_t R = (c >> 16) & 0xffu;
      const uint32_t G = (c >> 8) & 0xffu;
      const uint32_t B = c & 0xffu;
      const uint32_t lum = (R + G + B) / 3u;
      if (lum > 200u && a > 200u) {
        *dst = 0xff;  // eraser / white
      } else if (a >= 24u) {
        *dst = 0x00;  // ink (incl. light AA fringe)
      } else {
        continue;
      }
      if (px < outL) outL = px;
      if (py < outT) outT = py;
      if (px + 1 > outR) outR = px + 1;
      if (py + 1 > outB) outB = py + 1;
    }
  }
  env->ReleaseIntArrayElements(rgba, pixels, JNI_ABORT);

  if (outR <= outL || outB <= outT) return JNI_FALSE;

  HteinkDispArea area{};
  area.left = outL;
  area.top = outT;
  area.right = outR;
  area.bottom = outB;
  area.offset = 0;
  area.mode = gMode;

  ALOGI("DISPAREA submit [%d,%d-%d,%d] mode=%u (screen %dx%d panel %dx%d)", area.left,
        area.top, area.right, area.bottom, area.mode, screenW, screenH, gWidth, gHeight);

  const int rc = ioctl(gFd, kIocDisparea, &area);
  if (rc != 0) {
    ALOGW("DISPAREA failed ret=%d errno=%d", rc, errno);
    return JNI_FALSE;
  }
  ALOGI("DISPAREA ok");
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
