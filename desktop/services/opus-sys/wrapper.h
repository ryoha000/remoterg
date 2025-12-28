// Opus wrapper header for bindgen
#include <stdint.h>

// opus.h からの必要な関数宣言
#include "opus.h"

// CTL 用のヘルパー関数（可変長引数を回避）
static inline int opus_encoder_set_bitrate_wrapper(OpusEncoder *st, int32_t bitrate) {
    return opus_encoder_ctl(st, OPUS_SET_BITRATE_REQUEST, bitrate);
}

static inline int opus_encoder_set_complexity_wrapper(OpusEncoder *st, int32_t complexity) {
    return opus_encoder_ctl(st, OPUS_SET_COMPLEXITY_REQUEST, complexity);
}
