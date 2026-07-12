# btls-sys 0.5.6 does not disable BoringSSL assembly when the Windows ARM64
# host and target triples match, causing it to link x86-64 Apple objects.
set(OPENSSL_NO_ASM ON CACHE BOOL "Disable BoringSSL assembly on Windows ARM64" FORCE)
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreadedDLL" CACHE STRING "Use the dynamic MSVC runtime" FORCE)
