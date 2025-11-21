#pragma once

#include "common.h"

#include "componenthandler.h"

#include <unordered_map>

class PluginInstance {
public:
  PluginInstance();
  ~PluginInstance();

  bool init(const std::string &path, const std::string &id);
  void destroy();

  IOConfigutaion _io_config;
  IOConfigutaion get_io_config();

  Steinberg::Vst::ProcessContext *processContext();

  Steinberg::Vst::EventList *eventList(Steinberg::Vst::BusDirection direction,
                                       int which);
  Steinberg::Vst::ParameterChanges *
  parameterChanges(Steinberg::Vst::BusDirection direction, int which);

  bool load_plugin_from_class(VST3::Hosting::PluginFactory &factory,
                              VST3::Hosting::ClassInfo &classInfo);

  Steinberg::Vst::HostProcessData _processData = {};

  std::unordered_map<uint64_t, Steinberg::Vst::ParamID> midi_cc_mappings = {};

  void look_for_cc_mapping(MidiCC cc);

  void _destroy(bool decrementRefCount);

  std::vector<Steinberg::Vst::BusInfo> _inAudioBusInfos, _outAudioBusInfos;
  int _numInAudioBuses = 0, _numOutAudioBuses = 0;
  std::vector<Steinberg::Vst::BusInfo> _inEventBusInfos, _outEventBusInfos;
  int _numInEventBuses = 0, _numOutEventBuses = 0;

  std::vector<Steinberg::Vst::SpeakerArrangement> _inSpeakerArrs,
      _outSpeakerArrs;

  VST3::Hosting::Module::Ptr _module = nullptr;

  Steinberg::IPtr<Steinberg::Vst::IComponent> component = nullptr;
  Steinberg::IPtr<Steinberg::Vst::IAudioProcessor> audio_processor = nullptr;
  Steinberg::IPtr<Steinberg::Vst::IEditController> edit_controller = nullptr;

  ComponentHandler *component_handler = nullptr;

  Steinberg::Vst::ProcessSetup process_setup = {};
  Steinberg::Vst::ProcessContext _processContext = {};

  Steinberg::IPtr<Steinberg::IPlugView> _view = nullptr;

  const void *rust_side_vst3_instance_object = nullptr;

  std::string name;
  std::string vendor;
  std::string version;
  std::string id;

  Steinberg::Vst::IConnectionPoint *iConnectionPointComponent = nullptr;
  Steinberg::Vst::IConnectionPoint *iConnectionPointController = nullptr;

  static Steinberg::Vst::HostApplication *standard_plugin_context;
  static int standard_plugin_context_ref_count;
};
