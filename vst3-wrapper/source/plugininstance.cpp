#include "plugininstance.h"

#include <iostream>

using namespace Steinberg;
using namespace Steinberg::Vst;
using namespace Steinberg::Vst::ChannelContext;

const int MAX_BLOCK_SIZE = 4096 * 2;

bool PluginInstance::init(const std::string &path, const std::string &id) {
  _destroy(false);

  ++standard_plugin_context_ref_count;
  if (!standard_plugin_context) {
    standard_plugin_context = owned(NEW HostApplication());
    PluginContextFactory::instance().setPluginContext(standard_plugin_context);
  }

  process_setup.symbolicSampleSize = 0;
  process_setup.sampleRate = 44100;
  process_setup.maxSamplesPerBlock = MAX_BLOCK_SIZE;
  process_setup.processMode = Steinberg::Vst::kRealtime;

  _processData.numSamples = 0;
  _processData.processContext = &_processContext;

  std::string error;
  _module = VST3::Hosting::Module::create(path, error);
  if (!_module) {
    std::cerr << "Failed to load VST3 module: " << error << std::endl;
    return false;
  }

  VST3::Hosting::PluginFactory factory = _module->getFactory();
  for (auto &classInfo : factory.classInfos()) {
    if (classInfo.category() == kVstAudioEffectClass) {
      if (id != classInfo.ID().toString())
        continue;

      return this->load_plugin_from_class(factory, classInfo);
    }
  }

  std::cerr << "No plugin with the provided ID." << std::endl;
  return false;
}

bool PluginInstance::load_plugin_from_class(
    VST3::Hosting::PluginFactory &factory,
    VST3::Hosting::ClassInfo &classInfo) {
  component = factory.createInstance<Steinberg::Vst::IComponent>(classInfo.ID());
  if (!component)
    return false;

  if (component->initialize(standard_plugin_context) != kResultOk) {
    std::cout << "Failed to initialize component" << std::endl;
  }

  audio_processor = FUnknownPtr<IAudioProcessor>(component);
  if (!audio_processor) {
    std::cout << "Could not get audio processor from VST" << std::endl;
    return false;
  }

  auto res = component->queryInterface(Vst::IEditController::iid,
                                      (void **)&edit_controller);

  if (res != Steinberg::kResultOk) {
    TUID controllerCID;
    if (component->getControllerClassId(controllerCID) == kResultOk) {
      factory.get()->createInstance(controllerCID, Vst::IEditController::iid,
                                    (void **)&edit_controller);
    }
  }

  if (edit_controller->initialize(standard_plugin_context) != kResultOk) {
    std::cout << "Failed to initialize controller" << std::endl;
  }

  // https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/API+Documentation/Index.html#communication-between-the-components

  component_handler =
      new ComponentHandler(rust_side_vst3_instance_object);
  edit_controller->setComponentHandler((ComponentHandler *)component_handler);

  Vst::IConnectionPoint *iConnectionPointComponent = nullptr;
  Vst::IConnectionPoint *iConnectionPointController = nullptr;

  component->queryInterface(Vst::IConnectionPoint::iid,
                           (void **)&iConnectionPointComponent);
  edit_controller->queryInterface(Vst::IConnectionPoint::iid,
                                  (void **)&iConnectionPointController);

  if (iConnectionPointComponent && iConnectionPointController) {
    iConnectionPointComponent->connect(iConnectionPointController);
    iConnectionPointController->connect(iConnectionPointComponent);
  } else {
    std::cout << "Failed to get connection points." << std::endl;
  }

  auto stream = ResizableMemoryIBStream();
  // stream.setByteOrder(kLittleEndian);
  if (component->getState(&stream) == kResultTrue) {
    stream.rewind();
    edit_controller->setComponentState(&stream);
  }

  name = classInfo.name();
  vendor = classInfo.vendor();
  version = classInfo.version();
  id = classInfo.ID().toString();

  // TODO: Set bus arrangement

  _numInAudioBuses =
      component->getBusCount(MediaTypes::kAudio, BusDirections::kInput);
  _numOutAudioBuses =
      component->getBusCount(MediaTypes::kAudio, BusDirections::kOutput);
  _numInEventBuses =
      component->getBusCount(MediaTypes::kEvent, BusDirections::kInput);
  _numOutEventBuses =
      component->getBusCount(MediaTypes::kEvent, BusDirections::kOutput);

  for (int i = 0; i < _numInAudioBuses; ++i) {
    BusInfo info;
    component->getBusInfo(kAudio, kInput, i, info);
    _inAudioBusInfos.push_back(info);
    component->activateBus(kAudio, kInput, i, false);

    SpeakerArrangement speakerArr;
    audio_processor->getBusArrangement(kInput, i, speakerArr);
    _inSpeakerArrs.push_back(speakerArr);
  }

  for (int i = 0; i < _numInEventBuses; ++i) {
    BusInfo info;
    component->getBusInfo(kEvent, kInput, i, info);
    _inEventBusInfos.push_back(info);
    component->activateBus(kEvent, kInput, i, false);
  }

  for (int i = 0; i < _numOutAudioBuses; ++i) {
    BusInfo info;
    component->getBusInfo(kAudio, kOutput, i, info);
    _outAudioBusInfos.push_back(info);
    component->activateBus(kAudio, kOutput, i, false);

    SpeakerArrangement speakerArr;
    audio_processor->getBusArrangement(kOutput, i, speakerArr);
    _outSpeakerArrs.push_back(speakerArr);
  }

  for (int i = 0; i < _numOutEventBuses; ++i) {
    BusInfo info;
    component->getBusInfo(kEvent, kOutput, i, info);
    _outEventBusInfos.push_back(info);
    component->activateBus(kEvent, kOutput, i, false);
  }

  res = audio_processor->setBusArrangements(
      _inSpeakerArrs.data(), _numInAudioBuses, _outSpeakerArrs.data(),
      _numOutAudioBuses);
  if (res != kResultTrue) {
    std::cout << "Failed to set bus arrangements" << std::endl;
  }

  res = audio_processor->setupProcessing(process_setup);
  if (res == kResultOk) {
    _processData.prepare(*component, MAX_BLOCK_SIZE,
                         process_setup.symbolicSampleSize);
    if (_numInEventBuses > 0) {
      _processData.inputEvents = new EventList[_numInEventBuses];
    }
    if (_numOutEventBuses > 0) {
      _processData.outputEvents = new EventList[_numOutEventBuses];
    }
  } else {
    std::cout << "Failed to setup VST processing" << std::endl;
  }

  get_io_config();

  look_for_cc_mapping({0, 0, 129});

  return true;
}

void PluginInstance::look_for_cc_mapping(MidiCC cc) {
  ffi_ensure_main_thread("[VST3] look_for_cc_mapping");

  if (midi_cc_mappings.find(cc.as_key()) != midi_cc_mappings.end())
    return;

  IMidiMapping *midi_map = nullptr;
  edit_controller->queryInterface(IMidiMapping::iid, (void **)&midi_map);
  if (midi_map == nullptr)
    return;

  ParamID id = -1;

  // [UI-thread & Connected]
  if (midi_map->getMidiControllerAssignment(cc.bus_index, cc.channel,
                                            cc.control_number, id) != kResultOk)
    return;

  if (id == -1)
    return;

  midi_cc_mappings[cc.as_key()] = id;
}

void PluginInstance::destroy() { _destroy(true); }

void PluginInstance::_destroy(bool decrementRefCount) {
  // destroyView();

  edit_controller = nullptr;
  audio_processor = nullptr;
  component = nullptr;
  _module = nullptr;

  _inAudioBusInfos.clear();
  _outAudioBusInfos.clear();
  _numInAudioBuses = 0;
  _numOutAudioBuses = 0;

  _inEventBusInfos.clear();
  _outEventBusInfos.clear();
  _numInEventBuses = 0;
  _numOutEventBuses = 0;

  _inSpeakerArrs.clear();
  _outSpeakerArrs.clear();

  // if (_processData.inputEvents) {
  //   delete[] static_cast<Steinberg::Vst::EventList
  //   *>(_processData.inputEvents);
  // }
  // if (_processData.outputEvents) {
  //   delete[] static_cast<Steinberg::Vst::EventList *>(
  //       _processData.outputEvents);
  // }
  // _processData.unprepare();
  // _processData = {};
  //
  // _processSetup = {};
  // _processContext = {};

  // name = "";

  // if (decrementRefCount) {
  //   if (_standardPluginContextRefCount > 0) {
  //     --_standardPluginContextRefCount;
  //   }
  //   if (_standardPluginContext && _standardPluginContextRefCount == 0) {
  //     PluginContextFactory::instance().setPluginContext(nullptr);
  //     _standardPluginContext->release();
  //     delete _standardPluginContext;
  //     _standardPluginContext = nullptr;
  //   }
  // }
}
