cmake_minimum_required(VERSION 3.22.1)
project(latency_sink)

set(CMAKE_CXX_STANDARD 17)

add_library(latency_sink SHARED
    latency_sink.cpp
)

target_include_directories(latency_sink PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/third_party/webrtc_m124
)

# Enable release-style DCHECK behavior for this isolated JNI shim to avoid
# unresolved debug check symbols from inline WebRTC headers.
target_compile_definitions(latency_sink PRIVATE NDEBUG)

# Keep C++ virtual dispatch ABI aligned with ARM WebRTC binaries that expect
# relative vtable entries across the DSO boundary.
if(ANDROID_ABI STREQUAL "arm64-v8a" OR ANDROID_ABI STREQUAL "armeabi-v7a")
    target_compile_options(latency_sink PRIVATE -fexperimental-relative-c++-abi-vtables)
endif()

find_library(log-lib log)
target_link_libraries(latency_sink ${log-lib})
