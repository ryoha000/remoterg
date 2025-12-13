// libyuv wrapper header for bindgen
#include <stdint.h>

// libyuvの主要な変換関数を宣言
// ABGRToI420: ABGR (RGBA in memory order) -> I420 (YUV420)
int ABGRToI420(const uint8_t* src_abgr,
               int src_stride_abgr,
               uint8_t* dst_y,
               int dst_stride_y,
               uint8_t* dst_u,
               int dst_stride_u,
               uint8_t* dst_v,
               int dst_stride_v,
               int width,
               int height);

// ABGRToNV12: ABGR (RGBA in memory order) -> NV12 (Y plane + interleaved UV plane)
int ABGRToNV12(const uint8_t* src_abgr,
               int src_stride_abgr,
               uint8_t* dst_y,
               int dst_stride_y,
               uint8_t* dst_uv,
               int dst_stride_uv,
               int width,
               int height);
