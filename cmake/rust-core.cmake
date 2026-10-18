# Builds the Rust core (m8fmt + engine, bundled by ffi/) with cargo and exposes
# it as the imported target zerotracker::core.
#
# Cross builds (CMAKE_SYSTEM_PROCESSOR aarch64) pass --target to cargo; the
# linker comes from CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER, set in the
# trixie build container.

find_program(CARGO_EXECUTABLE cargo REQUIRED)

if(CMAKE_CROSSCOMPILING AND CMAKE_SYSTEM_PROCESSOR STREQUAL "aarch64")
    set(ZT_RUST_TARGET "aarch64-unknown-linux-gnu")
    set(ZT_RUST_TARGET_ARGS --target ${ZT_RUST_TARGET})
    set(ZT_RUST_OUT_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target/${ZT_RUST_TARGET}/release")
else()
    set(ZT_RUST_TARGET_ARGS "")
    set(ZT_RUST_OUT_DIR "${CMAKE_CURRENT_SOURCE_DIR}/target/release")
endif()

set(ZT_RUST_LIB "${ZT_RUST_OUT_DIR}/libzerotracker_core.a")

# Always run cargo: it is incremental and tracks Rust sources itself.
add_custom_target(zerotracker_rust_build ALL
    COMMAND ${CARGO_EXECUTABLE} build --release -p zt-ffi ${ZT_RUST_TARGET_ARGS}
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
    BYPRODUCTS "${ZT_RUST_LIB}"
    COMMENT "cargo build -p zt-ffi"
    USES_TERMINAL
)

add_library(zerotracker_rust STATIC IMPORTED GLOBAL)
set_target_properties(zerotracker_rust PROPERTIES
    IMPORTED_LOCATION "${ZT_RUST_LIB}"
    INTERFACE_INCLUDE_DIRECTORIES
        "${CMAKE_CURRENT_SOURCE_DIR}/ffi/include;${CMAKE_CURRENT_SOURCE_DIR}/engine/include"
    INTERFACE_LINK_LIBRARIES "pthread;dl;m"
)
add_dependencies(zerotracker_rust zerotracker_rust_build)
add_library(zerotracker::core ALIAS zerotracker_rust)
