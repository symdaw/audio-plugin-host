cmake_minimum_required(VERSION 3.10)

project(vst3wrapper)

set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreadedDDL")

set(CMAKE_CXX_STANDARD 17)

if(DEFINED ENV{VSTSDK_DIR})
    file(TO_CMAKE_PATH $ENV{VSTSDK_DIR} VST_SOURCE_DIR)
else()
    message(FATAL_ERROR "VSTSDK_DIR environment variable is not set.")
endif()

if (WIN32)
    set(PLATFORM_SPECIFIC
        ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/module_win32.cpp
    )
endif (WIN32)

if (LINUX)
    set(PLATFORM_SPECIFIC
        ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/module_linux.cpp
    )
endif (LINUX)

if (APPLE)
    set(PLATFORM_SPECIFIC
        ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/module_mac.mm
    )
endif (APPLE)

add_library(VST_SDK
    ${PLATFORM_SPECIFIC}
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/connectionproxy.cpp
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/eventlist.cpp
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/hostclasses.cpp
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/module.cpp
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/parameterchanges.cpp
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/pluginterfacesupport.cpp
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/plugprovider.cpp
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/hosting/processdata.cpp
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/utility/stringconvert.cpp
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/vst/vstinitiids.cpp
    ${VST_SOURCE_DIR}/vst3sdk/public.sdk/source/common/threadchecker_win32.cpp

    ${VST_SOURCE_DIR}/vst3sdk/pluginterfaces/base/conststringtable.cpp
    ${VST_SOURCE_DIR}/vst3sdk/pluginterfaces/base/coreiids.cpp
    ${VST_SOURCE_DIR}/vst3sdk/pluginterfaces/base/funknown.cpp
    ${VST_SOURCE_DIR}/vst3sdk/base/source/fobject.cpp
    ${VST_SOURCE_DIR}/vst3sdk/base/source/fdebug.cpp
    ${VST_SOURCE_DIR}/vst3sdk/base/source/updatehandler.cpp
    ${VST_SOURCE_DIR}/vst3sdk/base/thread/source/flock.cpp
)

target_include_directories(VST_SDK
    PUBLIC "${VST_SOURCE_DIR}/vst3sdk/"
)
target_compile_definitions(VST_SDK
    PUBLIC
        "-DRELEASE"
)

set(vst3wrapper_sources
    # ${SDK_ROOT}/public.sdk/source/vst/hosting/plugprovider.cpp
    # ${SDK_ROOT}/public.sdk/source/vst/hosting/plugprovider.h
    source/vst3wrapper.cpp
    source/vst3wrapper.h
    source/memoryibstream.h
)


set(target vst3wrapper)
add_library(${target} STATIC ${vst3wrapper_sources})
target_compile_features(${target}
    PUBLIC
        cxx_std_17
)

if (APPLE)
    set_property (TARGET VST_SDK APPEND_STRING PROPERTY 
                  COMPILE_FLAGS "-fobjc-arc")
endif (APPLE)

target_compile_definitions(vst3wrapper
    PUBLIC
        "-DSDL_MAIN_HANDLED"
)

target_link_libraries(vst3wrapper
    PRIVATE
        VST_SDK
	stdc++exp
)

target_include_directories(vst3wrapper
    PRIVATE
        include
)


