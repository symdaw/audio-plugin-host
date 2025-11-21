#pragma once

#include "vendor/memoryibstream.h"

#include <pluginterfaces/gui/iplugview.h>
#include <public.sdk/source/vst/hosting/eventlist.h>
#include <public.sdk/source/vst/hosting/parameterchanges.h>
#include <public.sdk/source/vst/hosting/processdata.h>
#include <pluginterfaces/vst/ivstchannelcontextinfo.h>
#include "public.sdk/source/vst/hosting/hostclasses.h"
#include "public.sdk/source/vst/hosting/module.h"
#include "public.sdk/source/vst/hosting/plugprovider.h"

#include "bindings.h"

struct ParameterEditState {
  int id;
  float initial_value;
  float current_value;
  bool finished;
};

struct MidiCC {
  int32_t bus_index;
  int16_t channel;
  int16_t control_number;

  inline uint64_t as_key() {
    return ((uint64_t)bus_index << 32) | ((uint64_t)bus_index << 16) | (uint64_t)channel;
  }
};

