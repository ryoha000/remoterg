/**
 * JNI LatencyVideoSink - VideoFrame::packet_infos().absolute_capture_time() から
 * キャプチャ時刻を読み取り、Kotlin へコールバックして E2E レイテンシを算出する。
 */
#include <jni.h>
#include <android/log.h>

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <mutex>

#include "api/video/video_frame.h"
#include "api/video/video_sink_interface.h"
#include "api/video_track_source_constraints.h"

#define LOG_TAG "LatencySink"
#define LOGD(...) __android_log_print(ANDROID_LOG_DEBUG, LOG_TAG, __VA_ARGS__)
#define LOGW(...) __android_log_print(ANDROID_LOG_WARN, LOG_TAG, __VA_ARGS__)

namespace {

JavaVM* g_jvm = nullptr;
thread_local JNIEnv* g_thread_env = nullptr;

// NTP epoch (1900-01-01) と Unix epoch (1970-01-01) の秒差
constexpr int64_t kNtpEpochOffsetSecs = 2208988800LL;
constexpr int64_t kMinUnixMs = 1'578'000'000'000LL;  // 2020-01
constexpr int64_t kMaxUnixMs = 2'050'000'000'000LL;  // 2034-12
constexpr uint64_t kFrameLogInterval = 120;

enum class ExtractStatus : int {
  kOk = 0,
  kNoPacketInfos = 1,
  kNoAbsoluteCaptureTime = 2,
  kOutOfRange = 3,
};

struct ExtractResult {
  bool ok = false;
  ExtractStatus status = ExtractStatus::kNoPacketInfos;
  size_t packet_info_count = 0;
  size_t packet_info_index = 0;
  int64_t capture_ntp_ms = 0;
  int64_t capture_unix_ms = 0;
  int64_t timestamp_us = 0;
};

/** NTP ミリ秒 → Unix ミリ秒 */
int64_t ntp_ms_to_unix_ms(int64_t ntp_ms) {
  int64_t ntp_secs = ntp_ms / 1000;
  int64_t unix_secs = ntp_secs - kNtpEpochOffsetSecs;
  int64_t remainder_ms = ntp_ms % 1000;
  if (remainder_ms < 0) {
    remainder_ms += 1000;
  }
  return unix_secs * 1000 + remainder_ms;
}

/** NTP UQ32.32 → ミリ秒 (NTP epoch 基準) */
int64_t ntp_uq32_to_ms(uint64_t ntp_ts) {
  const uint64_t secs = ntp_ts >> 32;
  const uint64_t frac = ntp_ts & 0xFFFFFFFFULL;
  const uint64_t frac_ms = (frac * 1000ULL) >> 32;
  return static_cast<int64_t>(secs * 1000ULL + frac_ms);
}

bool is_unix_plausible(int64_t unix_ms) {
  return unix_ms >= kMinUnixMs && unix_ms <= kMaxUnixMs;
}

JNIEnv* GetEnvForCurrentThread() {
  if (!g_jvm) {
    return nullptr;
  }

  JNIEnv* env = g_thread_env;
  if (env && g_jvm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) == JNI_OK) {
    return env;
  }

  env = nullptr;
  const jint get_env_result = g_jvm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6);
  if (get_env_result == JNI_OK && env) {
    g_thread_env = env;
    return env;
  }

  if (get_env_result == JNI_EDETACHED) {
    if (g_jvm->AttachCurrentThread(&env, nullptr) == JNI_OK && env) {
      // Detach は行わない。IncomingVideoSt スレッドで他 JNI 呼び出しと共有されるため。
      g_thread_env = env;
      return env;
    }
  }

  return nullptr;
}

ExtractResult ExtractCaptureTimeFromFrame(const webrtc::VideoFrame& frame) {
  ExtractResult result;
  result.timestamp_us = frame.timestamp_us();

  const auto& infos = frame.packet_infos();
  result.packet_info_count = infos.size();
  if (infos.empty()) {
    result.status = ExtractStatus::kNoPacketInfos;
    return result;
  }

  bool saw_absolute_capture_time = false;
  bool saw_out_of_range = false;
  for (size_t i = 0; i < infos.size(); ++i) {
    const auto& info = infos[i];
    const auto& act = info.absolute_capture_time();
    if (!act.has_value()) {
      continue;
    }
    saw_absolute_capture_time = true;

    const int64_t capture_ntp_ms = ntp_uq32_to_ms(act->absolute_capture_timestamp);

    const int64_t capture_unix_ms = ntp_ms_to_unix_ms(capture_ntp_ms);
    if (!is_unix_plausible(capture_unix_ms)) {
      saw_out_of_range = true;
      continue;
    }

    result.ok = true;
    result.status = ExtractStatus::kOk;
    result.packet_info_index = i;
    result.capture_ntp_ms = capture_ntp_ms;
    result.capture_unix_ms = capture_unix_ms;
    return result;
  }

  if (!saw_absolute_capture_time) {
    result.status = ExtractStatus::kNoAbsoluteCaptureTime;
  } else if (saw_out_of_range) {
    result.status = ExtractStatus::kOutOfRange;
  } else {
    result.status = ExtractStatus::kNoAbsoluteCaptureTime;
  }
  return result;
}

class LatencyVideoSink : public rtc::VideoSinkInterface<webrtc::VideoFrame> {
 public:
  LatencyVideoSink(JavaVM* jvm, JNIEnv* env, jobject callback)
      : jvm_(jvm) {
    if (!env || !callback) {
      return;
    }

    callback_ref_ = env->NewGlobalRef(callback);
    if (!callback_ref_) {
      return;
    }

    jclass callback_class = env->GetObjectClass(callback_ref_);
    if (!callback_class) {
      return;
    }

    on_capture_time_method_ = env->GetMethodID(callback_class, "onCaptureTime", "(IJJ)V");
    env->DeleteLocalRef(callback_class);

    if (!on_capture_time_method_) {
      LOGD("onCaptureTime(IJJ)V not found");
      if (env->ExceptionCheck()) {
        env->ExceptionClear();
      }
    } else {
      LOGD("LatencyVideoSink created successfully (callback method resolved)");
    }
  }

  ~LatencyVideoSink() override {
    shutting_down_.store(true, std::memory_order_release);

    std::lock_guard<std::mutex> lock(callback_mutex_);
    if (!callback_ref_ || !jvm_) {
      return;
    }

    JNIEnv* env = nullptr;
    bool attached_here = false;
    if (jvm_->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) == JNI_EDETACHED) {
      if (jvm_->AttachCurrentThread(&env, nullptr) == JNI_OK) {
        attached_here = true;
      }
    }

    if (env) {
      env->DeleteGlobalRef(callback_ref_);
    }
    callback_ref_ = nullptr;
    on_capture_time_method_ = nullptr;

    if (attached_here) {
      jvm_->DetachCurrentThread();
    }
  }

  void Shutdown() {
    shutting_down_.store(true, std::memory_order_release);
  }

  void OnFrame(const webrtc::VideoFrame& frame) override {
    if (shutting_down_.load(std::memory_order_acquire)) {
      return;
    }

    const uint64_t frame_no = frame_count_.fetch_add(1, std::memory_order_relaxed) + 1;
    const ExtractResult extract = ExtractCaptureTimeFromFrame(frame);
    if (!extract.ok) {
      const uint64_t fail_count =
          extract_fail_count_.fetch_add(1, std::memory_order_relaxed) + 1;
      switch (extract.status) {
        case ExtractStatus::kNoPacketInfos:
          extract_no_packet_infos_count_.fetch_add(1, std::memory_order_relaxed);
          break;
        case ExtractStatus::kNoAbsoluteCaptureTime:
          extract_no_abs_capture_time_count_.fetch_add(1, std::memory_order_relaxed);
          break;
        case ExtractStatus::kOutOfRange:
          extract_out_of_range_count_.fetch_add(1, std::memory_order_relaxed);
          break;
        case ExtractStatus::kOk:
          break;
      }

      if (fail_count <= 5 || frame_no % kFrameLogInterval == 1) {
        LOGW(
            "ACT skip frame=%llu status=%d success=%llu fail=%llu no_infos=%llu no_act=%llu out_of_range=%llu infos=%zu timestamp_us=%lld",
            static_cast<unsigned long long>(frame_no),
            static_cast<int>(extract.status),
            static_cast<unsigned long long>(
                extract_success_count_.load(std::memory_order_relaxed)),
            static_cast<unsigned long long>(fail_count),
            static_cast<unsigned long long>(
                extract_no_packet_infos_count_.load(std::memory_order_relaxed)),
            static_cast<unsigned long long>(
                extract_no_abs_capture_time_count_.load(std::memory_order_relaxed)),
            static_cast<unsigned long long>(
                extract_out_of_range_count_.load(std::memory_order_relaxed)),
            extract.packet_info_count,
            static_cast<long long>(extract.timestamp_us));
      }
      CallJava(extract.status, 0, extract.timestamp_us);
      return;
    }

    const uint64_t success_count =
        extract_success_count_.fetch_add(1, std::memory_order_relaxed) + 1;
    if (success_count <= 5 || frame_no % kFrameLogInterval == 1) {
      LOGD(
          "ACT frame=%llu success=%llu info_index=%zu infos=%zu capture_ntp_ms=%lld capture_unix_ms=%lld timestamp_us=%lld",
          static_cast<unsigned long long>(frame_no),
          static_cast<unsigned long long>(success_count),
          extract.packet_info_index,
          extract.packet_info_count,
          static_cast<long long>(extract.capture_ntp_ms),
          static_cast<long long>(extract.capture_unix_ms),
          static_cast<long long>(extract.timestamp_us));
    }

    CallJava(ExtractStatus::kOk, extract.capture_unix_ms, extract.timestamp_us);
  }

  void OnDiscardedFrame() override {}

  void OnConstraintsChanged(
      const webrtc::VideoTrackSourceConstraints& /*constraints*/) override {}

 private:
  void CallJava(ExtractStatus status, int64_t capture_unix_ms, int64_t timestamp_us) {
    if (shutting_down_.load(std::memory_order_acquire)) {
      return;
    }

    std::lock_guard<std::mutex> lock(callback_mutex_);
    if (shutting_down_.load(std::memory_order_acquire) || !callback_ref_ ||
        !on_capture_time_method_) {
      return;
    }

    JNIEnv* env = GetEnvForCurrentThread();
    if (!env) {
      return;
    }

    env->CallVoidMethod(callback_ref_, on_capture_time_method_,
                        static_cast<jint>(status),
                        static_cast<jlong>(capture_unix_ms),
                        static_cast<jlong>(timestamp_us));
    if (env->ExceptionCheck()) {
      env->ExceptionDescribe();
      env->ExceptionClear();
    }
  }

  JavaVM* jvm_;
  jobject callback_ref_ = nullptr;
  jmethodID on_capture_time_method_ = nullptr;
  std::atomic<bool> shutting_down_{false};
  std::atomic<uint64_t> frame_count_{0};
  std::atomic<uint64_t> extract_success_count_{0};
  std::atomic<uint64_t> extract_fail_count_{0};
  std::atomic<uint64_t> extract_no_packet_infos_count_{0};
  std::atomic<uint64_t> extract_no_abs_capture_time_count_{0};
  std::atomic<uint64_t> extract_out_of_range_count_{0};
  std::mutex callback_mutex_;
};

}  // namespace

extern "C" {

JNIEXPORT jlong JNICALL
Java_moe_ryoha_remoterg_webrtc_LatencyNativeSink_nativeCreateLatencySink(
    JNIEnv* env, jclass /*clazz*/, jobject callback) {
  if (!env || !callback) {
    return 0;
  }

  JavaVM* jvm = nullptr;
  if (env->GetJavaVM(&jvm) != JNI_OK || !jvm) {
    return 0;
  }
  g_jvm = jvm;

  auto* sink = new LatencyVideoSink(jvm, env, callback);
  LOGD("nativeCreateLatencySink: sink=%p", sink);
  return reinterpret_cast<jlong>(sink);
}

JNIEXPORT void JNICALL
Java_moe_ryoha_remoterg_webrtc_LatencyNativeSink_nativeAttachToTrack(
    JNIEnv* env, jclass /*clazz*/, jlong native_track, jlong native_sink) {
  if (!env || native_track == 0 || native_sink == 0) {
    return;
  }

  jclass video_track_class = env->FindClass("org/webrtc/VideoTrack");
  if (!video_track_class) {
    if (env->ExceptionCheck()) {
      env->ExceptionClear();
    }
    return;
  }

  jmethodID add_sink =
      env->GetStaticMethodID(video_track_class, "nativeAddSink", "(JJ)V");
  if (!add_sink) {
    LOGD("nativeAddSink not found");
    if (env->ExceptionCheck()) {
      env->ExceptionClear();
    }
    env->DeleteLocalRef(video_track_class);
    return;
  }

  env->CallStaticVoidMethod(video_track_class, add_sink, native_track, native_sink);
  if (env->ExceptionCheck()) {
    env->ExceptionDescribe();
    env->ExceptionClear();
    LOGD("nativeAddSink threw exception");
  }
  LOGD("nativeAttachToTrack success: track=%lld sink=%lld",
       static_cast<long long>(native_track), static_cast<long long>(native_sink));
  env->DeleteLocalRef(video_track_class);
}

JNIEXPORT void JNICALL
Java_moe_ryoha_remoterg_webrtc_LatencyNativeSink_nativeDetachFromTrack(
    JNIEnv* env, jclass /*clazz*/, jlong native_track, jlong native_sink) {
  if (!env || native_track == 0 || native_sink == 0) {
    return;
  }

  jclass video_track_class = env->FindClass("org/webrtc/VideoTrack");
  if (!video_track_class) {
    if (env->ExceptionCheck()) {
      env->ExceptionClear();
    }
    return;
  }

  jmethodID remove_sink =
      env->GetStaticMethodID(video_track_class, "nativeRemoveSink", "(JJ)V");
  if (!remove_sink) {
    if (env->ExceptionCheck()) {
      env->ExceptionClear();
    }
    env->DeleteLocalRef(video_track_class);
    return;
  }

  env->CallStaticVoidMethod(video_track_class, remove_sink, native_track, native_sink);
  if (env->ExceptionCheck()) {
    env->ExceptionDescribe();
    env->ExceptionClear();
    LOGD("nativeRemoveSink threw exception");
  }
  LOGD("nativeDetachFromTrack success: track=%lld sink=%lld",
       static_cast<long long>(native_track), static_cast<long long>(native_sink));
  env->DeleteLocalRef(video_track_class);
}

JNIEXPORT void JNICALL
Java_moe_ryoha_remoterg_webrtc_LatencyNativeSink_nativeDestroySink(
    JNIEnv* /*env*/, jclass /*clazz*/, jlong native_sink) {
  if (native_sink == 0) {
    return;
  }

  auto* sink = reinterpret_cast<LatencyVideoSink*>(native_sink);
  LOGD("nativeDestroySink: sink=%p", sink);
  sink->Shutdown();
  delete sink;
}

}  // extern "C"
