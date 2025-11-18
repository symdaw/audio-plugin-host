#include "vst3wrapper.h"

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <iostream>
#include <unordered_map>
#include <vector>

using namespace Steinberg;
using namespace Steinberg::Vst;
using namespace Steinberg::Vst::ChannelContext;

const char *alloc_string(const char *str) {
  if (str == nullptr) {
    return nullptr;
  }
  size_t len = strlen(str);
  char *copy = new char[len + 1];
  strcpy(copy, str);
  return copy;
}

void send_param_change_event(
    const void *rust_side_vst3_instance_object, int32_t id, float value,
    float initial_value,
    const std::unordered_map<ParamID, int> *parameter_indicies,
    bool end_edit = false) {
  PluginIssuedEvent event = {};
  event.tag = PluginIssuedEvent::Tag::Parameter;
  event.parameter = {};
  event.parameter._0 = {};

  int32_t param_index = -1;
  if (parameter_indicies) {
    auto it = parameter_indicies->find(id);
    if (it != parameter_indicies->end()) {
      param_index = (int32_t)it->second;
    }
  }

  event.parameter._0.parameter_id = id,
  event.parameter._0.parameter_index = param_index,
  event.parameter._0.current_value = value,
  event.parameter._0.end_edit = end_edit,
  event.parameter._0.initial_value = initial_value,
  send_event_to_host(&event, rust_side_vst3_instance_object);
}

class PlugFrame : public Steinberg::IPlugFrame {
public:
  const void *rust_side_vst3_instance_object = nullptr;

  PlugFrame(const void *_rust_side_vst3_instance_object) {
    rust_side_vst3_instance_object = _rust_side_vst3_instance_object;
  }

  Steinberg::tresult resizeView(Steinberg::IPlugView *view,
                                Steinberg::ViewRect *newSize) override {
    PluginIssuedEvent event = {};
    event.tag = PluginIssuedEvent::Tag::ResizeWindow;
    event.resize_window = {};
    event.resize_window._0 = (uintptr_t)newSize->getWidth();
    event.resize_window._1 = (uintptr_t)newSize->getHeight();

    send_event_to_host(&event, rust_side_vst3_instance_object);

    return Steinberg::kResultOk;
  }

  Steinberg::tresult queryInterface(const Steinberg::TUID /*_iid*/,
                                    void ** /*obj*/) override {
    return Steinberg::kNoInterface;
  }
  // we do not care here of the ref-counting. A plug-in call of release should
  // not destroy this class!
  Steinberg::uint32 addRef() override { return 1000; }
  Steinberg::uint32 release() override { return 1000; }
};

class ComponentHandler : public Steinberg::Vst::IComponentHandler {
public:
  std::vector<ParameterEditState> *param_edits = nullptr;
  std::mutex *param_edits_mutex = nullptr;
  const void *rust_side_vst3_instance_object = nullptr;
  const std::unordered_map<ParamID, int> *parameter_indicies = nullptr;

  ComponentHandler(
      std::vector<ParameterEditState> *_param_edits,
      std::mutex *_param_edits_mutex,
      const void *_rust_side_vst3_instance_object,
      const std::unordered_map<ParamID, int> *_parameter_indicies) {
    param_edits = _param_edits;
    param_edits_mutex = _param_edits_mutex;
    rust_side_vst3_instance_object = _rust_side_vst3_instance_object;
    parameter_indicies = _parameter_indicies;
  }

  Steinberg::tresult beginEdit(Steinberg::Vst::ParamID id) override {
    // TODO
    return Steinberg::kResultOk;
  }

  Steinberg::tresult
  performEdit(Steinberg::Vst::ParamID id,
              Steinberg::Vst::ParamValue valueNormalized) override {
    if (!param_edits || !param_edits_mutex) {
      std::cout << "Param editing state was no initilaized" << std::endl;
      return Steinberg::kResultFalse;
    }

    std::lock_guard<std::mutex> guard(*param_edits_mutex);

    for (ParameterEditState &param : *param_edits) {
      if (param.id != id)
        continue;

      param.current_value = valueNormalized;

      send_param_change_event(rust_side_vst3_instance_object, id,
                              valueNormalized, param.initial_value,
                              parameter_indicies);

      return Steinberg::kResultOk;
    }

    ParameterEditState state = {};
    state.id = id;
    state.finished = false;
    state.current_value = valueNormalized;
    state.initial_value = valueNormalized;

    param_edits->push_back(state);

    send_param_change_event(rust_side_vst3_instance_object, id, valueNormalized,
                            valueNormalized, parameter_indicies);

    return Steinberg::kResultOk;
  }

  Steinberg::tresult endEdit(Steinberg::Vst::ParamID id) override {
    std::lock_guard<std::mutex> guard(*param_edits_mutex);

    for (int i = 0; i < param_edits->size(); i++) {
      auto param = param_edits->at(i);
      if (param.id != id)
        continue;

      send_param_change_event(rust_side_vst3_instance_object, param.id,
                              param.current_value, param.initial_value,
                              parameter_indicies, true);

      param_edits->erase(std::next(param_edits->begin(), i));

      return Steinberg::kResultOk;
    }

    send_param_change_event(rust_side_vst3_instance_object, id, NAN, NAN,
                            parameter_indicies, true);

    return Steinberg::kResultOk;
  }

  Steinberg::tresult restartComponent(Steinberg::int32 flags) override {
    // TODO

    PluginIssuedEvent event = {};
    event.tag = PluginIssuedEvent::Tag::IOChanged;
    send_event_to_host(&event, rust_side_vst3_instance_object);

    return Steinberg::kResultOk;
  }

private:
  Steinberg::tresult queryInterface(const Steinberg::TUID /*_iid*/,
                                    void ** /*obj*/) override {
    return Steinberg::kNoInterface;
  }
  // we do not care here of the ref-counting. A plug-in call of release should
  // not destroy this class!
  Steinberg::uint32 addRef() override { return 1000; }
  Steinberg::uint32 release() override { return 1000; }
};

Steinberg::Vst::HostApplication *PluginInstance::_standardPluginContext =
    nullptr;
int PluginInstance::_standardPluginContextRefCount = 0;

PluginInstance::PluginInstance() {}

PluginInstance::~PluginInstance() { destroy(); }

const int MAX_BLOCK_SIZE = 4096 * 2;

void get_descriptors(const char *path,
                     HeaplessVec<FFIPluginDescriptor, 10> *plugins) {
  auto plugin_ctx = NEW HostApplication();
  PluginContextFactory::instance().setPluginContext(plugin_ctx);

  std::string error;
  auto module_ = VST3::Hosting::Module::create(path, error);
  if (!module_) {
    std::cerr << "Failed to load VST3 module: " << error << std::endl;
    return;
  }

  VST3::Hosting::PluginFactory factory = module_->getFactory();
  for (auto &classInfo : factory.classInfos()) {
    if (classInfo.category() == kVstAudioEffectClass) {
      if (plugins->count >= 10)
        break;

      std::string name = classInfo.name();
      std::string vendor = classInfo.vendor();
      std::string version = classInfo.version();
      std::string id = classInfo.ID().toString();

      plugins->data[plugins->count].value.name = alloc_string(name.c_str());
      plugins->data[plugins->count].value.version =
          alloc_string(version.c_str());
      plugins->data[plugins->count].value.vendor = alloc_string(vendor.c_str());
      plugins->data[plugins->count].value.id = alloc_string(id.c_str());

      plugins->count++;
    }
  }
}

bool PluginInstance::init(const std::string &path, const std::string &id) {
  _destroy(false);

  ++_standardPluginContextRefCount;
  if (!_standardPluginContext) {
    _standardPluginContext = owned(NEW HostApplication());
    PluginContextFactory::instance().setPluginContext(_standardPluginContext);
  }

  _processSetup.symbolicSampleSize = 0;
  _processSetup.sampleRate = 44100;
  _processSetup.maxSamplesPerBlock = MAX_BLOCK_SIZE;
  _processSetup.processMode = Steinberg::Vst::kRealtime;

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
  _vstPlug = factory.createInstance<Steinberg::Vst::IComponent>(classInfo.ID());
  if (!_vstPlug)
    return false;

  if (_vstPlug->initialize(_standardPluginContext) != kResultOk) {
    std::cout << "Failed to initialize component" << std::endl;
  }

  _audioEffect = FUnknownPtr<IAudioProcessor>(_vstPlug);
  if (!_audioEffect) {
    std::cout << "Could not get audio processor from VST" << std::endl;
    return false;
  }

  auto res = _vstPlug->queryInterface(Vst::IEditController::iid,
                                      (void **)&_editController);

  if (res != Steinberg::kResultOk) {
    TUID controllerCID;
    if (_vstPlug->getControllerClassId(controllerCID) == kResultOk) {
      factory.get()->createInstance(controllerCID, Vst::IEditController::iid,
                                    (void **)&_editController);
    }
  }

  if (_editController->initialize(_standardPluginContext) != kResultOk) {
    std::cout << "Failed to initialize controller" << std::endl;
  }

  param_edits = {};

  // https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/API+Documentation/Index.html#communication-between-the-components

  component_handler =
      new ComponentHandler(&param_edits, &param_edits_mutex,
                           rust_side_vst3_instance_object, &parameter_indicies);
  _editController->setComponentHandler((ComponentHandler *)component_handler);

  Vst::IConnectionPoint *iConnectionPointComponent = nullptr;
  Vst::IConnectionPoint *iConnectionPointController = nullptr;

  _vstPlug->queryInterface(Vst::IConnectionPoint::iid,
                           (void **)&iConnectionPointComponent);
  _editController->queryInterface(Vst::IConnectionPoint::iid,
                                  (void **)&iConnectionPointController);

  if (iConnectionPointComponent && iConnectionPointController) {
    iConnectionPointComponent->connect(iConnectionPointController);
    iConnectionPointController->connect(iConnectionPointComponent);
  } else {
    std::cout << "Failed to get connection points." << std::endl;
  }

  auto stream = ResizableMemoryIBStream();
  // stream.setByteOrder(kLittleEndian);
  if (_vstPlug->getState(&stream) == kResultTrue) {
    stream.rewind();
    _editController->setComponentState(&stream);
  }

  name = classInfo.name();
  vendor = classInfo.vendor();
  version = classInfo.version();
  id = classInfo.ID().toString();

  // TODO: Set bus arrangement

  _numInAudioBuses =
      _vstPlug->getBusCount(MediaTypes::kAudio, BusDirections::kInput);
  _numOutAudioBuses =
      _vstPlug->getBusCount(MediaTypes::kAudio, BusDirections::kOutput);
  _numInEventBuses =
      _vstPlug->getBusCount(MediaTypes::kEvent, BusDirections::kInput);
  _numOutEventBuses =
      _vstPlug->getBusCount(MediaTypes::kEvent, BusDirections::kOutput);

  for (int i = 0; i < _numInAudioBuses; ++i) {
    BusInfo info;
    _vstPlug->getBusInfo(kAudio, kInput, i, info);
    _inAudioBusInfos.push_back(info);
    _vstPlug->activateBus(kAudio, kInput, i, false);

    SpeakerArrangement speakerArr;
    _audioEffect->getBusArrangement(kInput, i, speakerArr);
    _inSpeakerArrs.push_back(speakerArr);
  }

  for (int i = 0; i < _numInEventBuses; ++i) {
    BusInfo info;
    _vstPlug->getBusInfo(kEvent, kInput, i, info);
    _inEventBusInfos.push_back(info);
    _vstPlug->activateBus(kEvent, kInput, i, false);
  }

  for (int i = 0; i < _numOutAudioBuses; ++i) {
    BusInfo info;
    _vstPlug->getBusInfo(kAudio, kOutput, i, info);
    _outAudioBusInfos.push_back(info);
    _vstPlug->activateBus(kAudio, kOutput, i, false);

    SpeakerArrangement speakerArr;
    _audioEffect->getBusArrangement(kOutput, i, speakerArr);
    _outSpeakerArrs.push_back(speakerArr);
  }

  for (int i = 0; i < _numOutEventBuses; ++i) {
    BusInfo info;
    _vstPlug->getBusInfo(kEvent, kOutput, i, info);
    _outEventBusInfos.push_back(info);
    _vstPlug->activateBus(kEvent, kOutput, i, false);
  }

  res = _audioEffect->setBusArrangements(
      _inSpeakerArrs.data(), _numInAudioBuses, _outSpeakerArrs.data(),
      _numOutAudioBuses);
  if (res != kResultTrue) {
    std::cout << "Failed to set bus arrangements" << std::endl;
  }

  res = _audioEffect->setupProcessing(_processSetup);
  if (res == kResultOk) {
    _processData.prepare(*_vstPlug, MAX_BLOCK_SIZE,
                         _processSetup.symbolicSampleSize);
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
  _editController->queryInterface(IMidiMapping::iid, (void **)&midi_map);
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

uint32_t get_latency(const void *app) {
  ffi_ensure_main_thread("[VST3] get_latency");

  PluginInstance *vst = (PluginInstance *)app;

  // https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/Workflow+Diagrams/Get+Latency+Call+Sequence.html

  // [(UI-thread or processing-thread) & Activated]
  vst->_audioEffect->setProcessing(false);

  // [UI-thread & Setup Done]
  vst->_vstPlug->setActive(false);

  // Gets and sends tail length changed update. This should eventually be done
  // somewhere else. [UI-thread & Setup Done]
  uint32_t tail = vst->_audioEffect->getTailSamples();
  PluginIssuedEvent event = {};
  event.tag = PluginIssuedEvent::Tag::TailLengthChanged;
  event.tail_length_changed = {};
  event.tail_length_changed._0 = tail;
  send_event_to_host(&event, vst->rust_side_vst3_instance_object);

  vst->_vstPlug->setActive(true);

  // [(UI-thread or processing-thread) & Activated]
  uint32_t latency = vst->_audioEffect->getLatencySamples();

  // [(UI-thread or processing-thread) & Activated]
  vst->_audioEffect->setProcessing(true);
  return latency;
}

void set_processing(const void *app, bool processing) {
  // TODO: Ensure activated

  PluginInstance *vst = (PluginInstance *)app;

  // [(UI-thread or processing-thread) & Activated]
  vst->_audioEffect->setProcessing(processing);
}

Steinberg::Vst::ProcessContext *PluginInstance::processContext() {
  return &_processContext;
}

Steinberg::Vst::EventList *
PluginInstance::eventList(Steinberg::Vst::BusDirection direction, int which) {
  if (direction == kInput) {
    return static_cast<Steinberg::Vst::EventList *>(
        &_processData.inputEvents[which]);
  } else if (direction == kOutput) {
    return static_cast<Steinberg::Vst::EventList *>(
        &_processData.outputEvents[which]);
  } else {
    return nullptr;
  }
}

Steinberg::Vst::ParameterChanges *
PluginInstance::parameterChanges(Steinberg::Vst::BusDirection direction,
                                 int which) {
  if (direction == kInput) {
    return static_cast<Steinberg::Vst::ParameterChanges *>(
        &_processData.inputParameterChanges[which]);
  } else if (direction == kOutput) {
    return static_cast<Steinberg::Vst::ParameterChanges *>(
        &_processData.outputParameterChanges[which]);
  } else {
    return nullptr;
  }
}

IOConfigutaion PluginInstance::get_io_config() {
  IOConfigutaion io_config = {};
  io_config.audio_inputs = {};
  io_config.audio_outputs = {};

  auto audio_inputs =
      _vstPlug->getBusCount(MediaTypes::kAudio, BusDirections::kInput);
  auto audio_outputs =
      _vstPlug->getBusCount(MediaTypes::kAudio, BusDirections::kOutput);
  auto event_inputs =
      _vstPlug->getBusCount(MediaTypes::kEvent, BusDirections::kInput);

  for (int i = 0; i < audio_inputs; i++) {
    BusInfo info;
    _vstPlug->getBusInfo(MediaTypes::kAudio, BusDirections::kInput, i, info);
    io_config.audio_inputs.count++;
    io_config.audio_inputs.data[i] = {};
    io_config.audio_inputs.data[i].value.channels = info.channelCount;
  }

  for (int i = 0; i < audio_outputs; i++) {
    BusInfo info;
    _vstPlug->getBusInfo(MediaTypes::kAudio, BusDirections::kOutput, i, info);
    io_config.audio_outputs.count++;
    io_config.audio_outputs.data[i] = {};
    io_config.audio_outputs.data[i].value.channels = info.channelCount;
  }

  io_config.event_inputs_count = event_inputs;

  // vst->_vstPlug->getBusCount(MediaTypes::kEvent, BusDirections::kInput);
  // vst->_vstPlug->getBusCount(MediaTypes::kEvent, BusDirections::kOutput);

  _io_config = io_config;

  return io_config;
}

void PluginInstance::_destroy(bool decrementRefCount) {
  // destroyView();

  _editController = nullptr;
  _audioEffect = nullptr;
  _vstPlug = nullptr;
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
  //   delete[] static_cast<Steinberg::Vst::EventList *>(_processData.inputEvents);
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

const void *load_plugin(const char *s, const char *id,
                        const void *rust_side_vst3_instance_object) {
  PluginInstance *vst = new PluginInstance();
  vst->rust_side_vst3_instance_object = rust_side_vst3_instance_object;
  vst->init(s, id);


  auto aud_in = vst->_vstPlug->getBusCount(kAudio, kInput);
  for (int i = 0; i < aud_in; i++) {
    vst->_vstPlug->activateBus(kAudio, kInput, i, true);
  }

  auto aud_out = vst->_vstPlug->getBusCount(kAudio, kOutput);
  for (int i = 0; i < aud_out; i++) {
    vst->_vstPlug->activateBus(kAudio, kOutput, i, true);
  }

  auto evt_in = vst->_vstPlug->getBusCount(kEvent, kInput);
  for (int i = 0; i < evt_in; i++) {
    vst->_vstPlug->activateBus(kEvent, kInput, i, true);
  }

  if (vst->_vstPlug->setActive(true) != kResultTrue) {
    std::cout << "Failed to activate VST component" << std::endl;
  }

  if (vst->_audioEffect->setProcessing(true)) {
    std::cout << "Failed to being processing" << std::endl;
  }

  // NOTE: Output event buses are not supported yet so they are not activated

  return vst;
}

Dims show_gui(const void *app, const void *window_id,
              WindowIDType window_id_type) {
  PluginInstance *vst = (PluginInstance *)app;

  if (!vst->_editController) {
    std::cerr << "VST does not provide an edit controller" << std::endl;
    return {};
  }

  if (!vst->_view) {
    vst->_view = vst->_editController->createView(ViewType::kEditor);
    if (!vst->_view) {
      std::cerr << "EditController does not provide its own view" << std::endl;
      return {};
    }

    vst->_view->setFrame(
        owned(new PlugFrame(vst->rust_side_vst3_instance_object)));
  }

  auto platform = Steinberg::kPlatformTypeHWND;

  switch (window_id_type) {
  case WindowIDType::HWND:
    platform = kPlatformTypeHWND;
    break;
  case WindowIDType::NSView:
    platform = kPlatformTypeNSView;
    break;
  case WindowIDType::XWNDX11:
    platform = kPlatformTypeX11EmbedWindowID;
    break;
  }

  if (vst->_view->isPlatformTypeSupported(platform) != Steinberg::kResultTrue) {
    std::cerr << "Editor view does not support this platform" << std::endl;
    return {};
  }

  if (vst->_view->attached((void *)window_id, platform) !=
      Steinberg::kResultOk) {
    std::cerr << "Failed to attach editor view to view" << std::endl;
    return {};
  }

  ViewRect viewRect = {};
  if (vst->_view->getSize(&viewRect) != kResultOk) {
    std::cout << "Failed to get editor view size" << std::endl;
    return {};
  }

  return {
      viewRect.getWidth(),
      viewRect.getHeight(),
  };
}

void hide_gui(const void *app) {
  PluginInstance *vst = (PluginInstance *)app;
  if (vst->_view != nullptr) {
    vst->_view->release();
    vst->_view = nullptr;
  }
}

FFIPluginDescriptor descriptor(const void *app) {
  PluginInstance *vst = (PluginInstance *)app;

  FFIPluginDescriptor desc = {};
  desc.name = alloc_string(vst->name.c_str());
  desc.version = alloc_string(vst->version.c_str());
  desc.vendor = alloc_string(vst->vendor.c_str());
  desc.id = alloc_string(vst->id.c_str());

  return desc;
}

void vst3_set_sample_rate(const void *app, int32_t rate) {
  ffi_ensure_main_thread("[VST3] vst3_set_sample_rate");

  PluginInstance *vst = (PluginInstance *)app;

  // [(UI-thread or processing-thread) & Activated]
  vst->_audioEffect->setProcessing(false);

  // [UI-thread & Setup Done]
  vst->_vstPlug->setActive(false);

  vst->_processSetup.sampleRate = rate;

  // [UI-thread & (Initialized | Connected)]]
  vst->_audioEffect->setupProcessing(vst->_processSetup);

  // [UI-thread & Setup Done]
  vst->_vstPlug->setActive(true);

  // [(UI-thread or processing-thread) & Activated]
  vst->_audioEffect->setProcessing(true);

  vst->_processData.processContext->sampleRate = rate;
}

const void *get_data(const void *app, int32_t *data_len, const void **stream) {
  PluginInstance *vst = (PluginInstance *)app;

  ResizableMemoryIBStream *stream_ = new ResizableMemoryIBStream();
  *stream = stream_;

  if (vst->_vstPlug->getState(stream_) != kResultOk) {
    std::cerr << "Failed to get processor state." << std::endl;
    return nullptr;
  }

  Steinberg::int64 length = 0;
  stream_->tell(&length);
  *data_len = (int)length;

  stream_->rewind();

  return stream_->getData();
}

const void *get_controller_data(const void *app, int32_t *data_len,
                                const void **stream) {
  ffi_ensure_main_thread("[VST3] get_controller_data");

  PluginInstance *vst = (PluginInstance *)app;

  ResizableMemoryIBStream *stream_ = new ResizableMemoryIBStream();
  *stream = stream_;

  // [UI-thread & Connected]
  if (vst->_editController->getState(stream_) != kResultOk) {
    std::cerr << "Failed to get controller state." << std::endl;
    return nullptr;
  }

  Steinberg::int64 length = 0;
  stream_->tell(&length);
  *data_len = (int)length;

  stream_->rewind();

  return stream_->getData();
}

void free_data_stream(const void *stream) {
  ResizableMemoryIBStream *stream_ = (ResizableMemoryIBStream *)stream;
  delete stream_;
}

void set_data(const void *app, const void *data, int32_t data_len) {
  ffi_ensure_main_thread("[VST3] set_data");

  if (data_len == 0)
    return;

  // https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/API+Documentation/Index.html#persistence

  PluginInstance *vst = (PluginInstance *)app;

  ResizableMemoryIBStream stream(data_len);
  stream.rewind();

  int num_bytes_written = 0;
  stream.write((void *)data, data_len, &num_bytes_written);
  stream.rewind();
  assert(data_len == num_bytes_written);

  // for (int i = 0; i < data_len; i++) {
  //   std::cout << (int)((uint8_t *)data)[i] << std::endl;
  // }

  // [UI-thread & (Initialized | Connected | Setup Done | Activated |
  // Processing)]
  if (vst->_vstPlug->setState(&stream) != kResultOk) {
    std::cerr << "Failed to set processor state" << std::endl;
  }

  stream.rewind();

  // [UI-thread & Connected]
  if (vst->_editController->setComponentState(&stream) != kResultOk) {
    std::cerr << "Failed to set processor state in controller" << std::endl;
  }
}

void set_controller_data(const void *app, const void *data, int32_t data_len) {
  ffi_ensure_main_thread("[VST3] set_controller_data");

  if (data_len == 0)
    return;

  // https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/API+Documentation/Index.html#persistence

  PluginInstance *vst = (PluginInstance *)app;

  ResizableMemoryIBStream stream(data_len);
  stream.rewind();

  int num_bytes_written = 0;
  stream.write((void *)data, data_len, &num_bytes_written);
  stream.rewind();
  assert(data_len == num_bytes_written);

  // [UI-thread & Connected]
  if (vst->_editController->setState(&stream) != kResultOk) {
    std::cout << "Failed to set controller state" << std::endl;
  }
}

void process(const void *app, const ProcessDetails *data, float ***input,
             float ***output, HostIssuedEvent *events, int32_t events_len) {
  ffi_ensure_non_main_thread("[VST3] process");
  PluginInstance *vst = (PluginInstance *)app;

  auto audio_inputs = vst->_io_config.audio_inputs.count;
  auto audio_outputs = vst->_io_config.audio_outputs.count;

  vst->_processData.numSamples = data->block_size;

  for (int i = 0; i < audio_inputs; i++) {
    vst->_processData.inputs[i].numChannels =
        vst->_io_config.audio_inputs.data[i].value.channels;
    vst->_processData.inputs[i].silenceFlags = 0;
    vst->_processData.inputs[i].channelBuffers32 = input[i];
  }

  vst->_processData.numInputs = audio_inputs;

  for (int i = 0; i < audio_outputs; i++) {
    vst->_processData.outputs[i].numChannels =
        vst->_io_config.audio_outputs.data[i].value.channels;
    vst->_processData.outputs[i].silenceFlags = 0;
    vst->_processData.outputs[i].channelBuffers32 = output[i];
  }

  vst->_processData.numOutputs = audio_outputs;

  Steinberg::uint32 state = 0;

  Steinberg::Vst::ProcessContext *ctx = vst->_processData.processContext;

  vst->_processData.processContext->tempo = data->tempo;
  state |= ctx->kTempoValid;

  vst->_processData.processContext->timeSigNumerator =
      data->time_signature_numerator;
  vst->_processData.processContext->timeSigDenominator =
      data->time_signature_denominator;
  state |= ctx->kTimeSigValid;

  vst->_processData.processContext->projectTimeMusic = data->player_time;

  vst->_processData.processContext->projectTimeSamples =
      (data->player_time / (data->tempo / 60.)) * data->sample_rate;

  // TODO
  // vst->_processData.processContext->barPositionMusic = data.barPosBeats;
  // state |= ctx->kBarPositionValid;

  vst->_processData.processContext->cycleStartMusic = data->cycle_start;
  vst->_processData.processContext->cycleEndMusic = data->cycle_end;
  state |= ctx->kCycleValid;

  vst->_processData.processContext->systemTime = data->nanos;
  state |= ctx->kSystemTimeValid;

  vst->_processData.processContext->frameRate.framesPerSecond = 60.;
  vst->_processData.processContext->frameRate.flags = 0;

  if (data->cycle_enabled) {
    state |= ctx->kCycleActive;
  }

  if (data->playing_state != PlayingState::Stopped) {
    state |= ctx->kPlaying;
  }

  if (data->playing_state == PlayingState::Recording) {
    state |= ctx->kRecording;
  }

  if (data->playing_state == PlayingState::OfflineRendering) {
    vst->_processData.processMode = kOffline;
  } else {
    vst->_processData.processMode = kRealtime;
  }

  vst->_processData.processContext->state = state;

  int midi_bus = 0;
  Steinberg::Vst::EventList *eventList = nullptr;

  if (!vst->_processData.inputParameterChanges) {
    vst->_processData.inputParameterChanges = new ParameterChanges(400);
  }

  if (vst->_io_config.event_inputs_count > 0) {
    eventList = vst->eventList(Steinberg::Vst::kInput, midi_bus);

    for (int i = 0; i < events_len; i++) {
      auto tag = events[i].event_type.tag;
      if (tag != HostIssuedEventType::Tag::Midi &&
          tag != HostIssuedEventType::Tag::NoteExpression)
        continue;

      Steinberg::Vst::Event evt = {};
      evt.busIndex = midi_bus;
      evt.sampleOffset = events[i].block_time;
      evt.ppqPosition = events[i].ppq_time;

      if (events[i].is_live) {
        evt.flags |= Steinberg::Vst::Event::EventFlags::kIsLive;
      }

      if (tag == HostIssuedEventType::Tag::NoteExpression) {
        evt.type = Steinberg::Vst::Event::EventTypes::kNoteExpressionValueEvent;
        evt.noteExpressionValue.value =
            (Steinberg::Vst::NoteExpressionValue)events[i]
                .event_type.note_expression.value;
        evt.noteExpressionValue.noteId =
            (int32_t)events[i].event_type.note_expression.note_id;

        switch (events[i].event_type.note_expression.expression_type) {
        case NoteExpressionType::Volume:
          evt.noteExpressionValue.typeId =
              Steinberg::Vst::NoteExpressionTypeIDs::kVolumeTypeID;
          break;
        case NoteExpressionType::Pan:
          evt.noteExpressionValue.typeId =
              Steinberg::Vst::NoteExpressionTypeIDs::kPanTypeID;
          break;
        case NoteExpressionType::Tuning:
          evt.noteExpressionValue.typeId =
              Steinberg::Vst::NoteExpressionTypeIDs::kTuningTypeID;
          break;
        case NoteExpressionType::Vibrato:
          evt.noteExpressionValue.typeId =
              Steinberg::Vst::NoteExpressionTypeIDs::kVibratoTypeID;
          break;
        case NoteExpressionType::Brightness:
          evt.noteExpressionValue.typeId =
              Steinberg::Vst::NoteExpressionTypeIDs::kBrightnessTypeID;
          break;
        case NoteExpressionType::Expression:
          evt.noteExpressionValue.typeId =
              Steinberg::Vst::NoteExpressionTypeIDs::kExpressionTypeID;
          break;
        }

        eventList->addEvent(evt);
      }

      if (tag == HostIssuedEventType::Tag::Midi) {
        bool is_note_on = events[i].event_type.midi._0.midi_data[0] == 0x90;
        bool is_note_off = events[i].event_type.midi._0.midi_data[0] == 0x80;
        bool is_pitch_bend = events[i].event_type.midi._0.midi_data[0] == 0xE0;

        if (is_note_on) {
          evt.type = Steinberg::Vst::Event::EventTypes::kNoteOnEvent;
          evt.noteOn.channel = 0;
          evt.noteOn.pitch = events[i].event_type.midi._0.midi_data[1];
          evt.noteOn.tuning = events[i].event_type.midi._0.detune;
          evt.noteOn.velocity =
              (float)(events[i].event_type.midi._0.midi_data[2]) / 127.;
          evt.noteOn.length = 0;
          evt.noteOn.noteId = events[i].event_type.midi._0.note_id;
          eventList->addEvent(evt);
        } else if (is_note_off) {
          evt.type = Steinberg::Vst::Event::EventTypes::kNoteOffEvent;
          evt.noteOff.channel = 0;
          evt.noteOff.pitch = events[i].event_type.midi._0.midi_data[1];
          evt.noteOff.tuning = events[i].event_type.midi._0.detune;
          evt.noteOff.velocity =
              (float)(events[i].event_type.midi._0.midi_data[2]) / 127.;
          evt.noteOff.noteId = events[i].event_type.midi._0.note_id;
          eventList->addEvent(evt);
        } else if (is_pitch_bend) {
          MidiCC cc = {0, 0, 129};
          if (vst->midi_cc_mappings.find(cc.as_key()) !=
              vst->midi_cc_mappings.end()) {
            ParamID id = vst->midi_cc_mappings[cc.as_key()];

            auto changes = vst->_processData.inputParameterChanges;

            int queue_index = 0;
            auto queue = changes->addParameterData(id, queue_index);

            auto q = static_cast<ParameterValueQueue *>(queue);
            q->clear();

            float value =
                (float)((events[i].event_type.midi._0.midi_data[2] << 7) |
                        (events[i].event_type.midi._0.midi_data[1])) /
                (float)0x4000;

            int point_index = 0;
            if (queue->addPoint(events[i].block_time, value, point_index) !=
                kResultOk) {
              std::cout << "Failed to set pitch bend" << std::endl;
            }
          }
        } else {
          evt.type = Steinberg::Vst::Event::EventTypes::kDataEvent;
          evt.data.size = 3;
          evt.data.type = Steinberg::Vst::DataEvent::DataTypes::kMidiSysEx;
          evt.data.bytes = events[i].event_type.midi._0.midi_data;
          std::cout << (int)events[i].event_type.midi._0.midi_data[1] << " "
                    << (int)events[i].event_type.midi._0.midi_data[2]
                    << std::endl;
          eventList->addEvent(evt);
        }
      }
    }
  }

  for (int i = 0; i < events_len; i++) {
    if (events[i].event_type.tag != HostIssuedEventType::Tag::Parameter)
      continue;

    auto changes = vst->_processData.inputParameterChanges;

    auto time = events[i].block_time;
    auto id = events[i].event_type.parameter._0.parameter_id;
    auto value = events[i].event_type.parameter._0.current_value;

    int queue_index = 0;
    auto queue = changes->addParameterData(id, queue_index);

    auto q = static_cast<ParameterValueQueue *>(queue);
    q->clear();

    int point_index = 0;
    if (queue->addPoint(time, value, point_index) != kResultOk) {
      std::cout << "Failed to set parameter" << std::endl;
    }
  }

  // [processing-thread & Processing]
  tresult result = vst->_audioEffect->process(vst->_processData);
  if (result != kResultOk) {
    std::cout << "Failed to process" << std::endl;
  }

  if (eventList) {
    eventList->clear();
  }
}

void set_track_details(const void *app, const Track *details) {
  ffi_ensure_main_thread("[VST3] set_track_details");

  PluginInstance *vst = (PluginInstance *)app;

  IInfoListener *track_info_listener = nullptr;
  vst->_editController->queryInterface(IInfoListener::iid,
                                       (void **)&track_info_listener);
  if (track_info_listener == nullptr)
    return;

  auto list = HostAttributeList::make();

  // https://github.com/steinbergmedia/vst3_pluginterfaces/blob/dd77488d3dc329c484b5dfb47af9383356e4c0cc/vst/ivstchannelcontextinfo.h#L189-L208
  uint64_t col = 0;
  col |= (uint64_t)details->col.b;
  col |= (uint64_t)details->col.g << 8;
  col |= (uint64_t)details->col.r << (8 * 2);
  col |= (uint64_t)details->col.a << (8 * 3);

  list->setInt(ChannelContext::kChannelColorKey, col);

  TChar name[64] = {};
  for (int i = 0; i < details->name.data.count; i++) {
    name[i] = (TChar)details->name.data.data[i].value;
  }

  list->setString(ChannelContext::kChannelNameKey, name);
  list->setInt(ChannelContext::kChannelNameLengthKey, details->name.data.count);

  // [UI-thread & (Initialized | Connected | Setup Done | Activated |
  // Processing)]
  track_info_listener->setChannelContextInfos(list);
}

void set_param_in_edit_controller(const void *app, int32_t id, float value) {
  PluginInstance *vst = (PluginInstance *)app;

  // Takes param id
  if (vst->_editController->setParamNormalized(id, value) != kResultOk) {
    std::cout << "Failed to set parameter normalized" << std::endl;
  }
}

void free_string(const char *str) { delete[] str; }

Parameter get_parameter(const void *app, int32_t index) {
  // TODO: sort out naming confusion with id and index

  ffi_ensure_main_thread("[VST3] get_parameter");

  PluginInstance *vst = (PluginInstance *)app;

  ParameterInfo param_info = {};
  // Takes index
  vst->_editController->getParameterInfo(index, param_info);

  vst->parameter_indicies[param_info.id] = index;

  // TODO: Make real-time safe with stack buffers

  std::string name = {};
  for (TChar c : param_info.title) {
    if (c != '\0') {
      name += c;
    }
  }

  Steinberg::Vst::ParamValue value =
      vst->_editController->getParamNormalized(param_info.id);

  TChar formatted_value[128] = {};
  if (vst->_editController->getParamStringByValue(
          param_info.id, value, formatted_value) != kResultOk) {
    std::cout << "Failed to get parameter value by string" << std::endl;
  }

  std::string formatted_value_c_str = {};
  for (TChar c : formatted_value) {
    if (c != '\0') {
      formatted_value_c_str += c;
    }
  }

  Parameter param = {};
  param.id = param_info.id;
  param.index = index;
  param.value = (float)value;

  push_c_str_to_heapless_string(&param.name, name.c_str());

  push_c_str_to_heapless_string(&param.formatted_value,
                                formatted_value_c_str.c_str());

  param.is_wrap_around = (param_info.flags & ParameterInfo::kIsWrapAround) != 0;
  param.hidden = (param_info.flags & ParameterInfo::kIsHidden) != 0;
  param.can_automate = (param_info.flags & ParameterInfo::kCanAutomate) != 0;
  param.read_only = (param_info.flags & ParameterInfo::kIsReadOnly) != 0;

  param.default_value = (float)param_info.defaultNormalizedValue;

  return param;
}

IOConfigutaion io_config(const void *app) {
  PluginInstance *vst = (PluginInstance *)app;

  return vst->get_io_config();
}

uintptr_t parameter_count(const void *app) {
  ffi_ensure_main_thread("[VST3] parameter_count");

  auto vst = (PluginInstance *)app;

  // [UI-thread & Connected]
  return vst->_editController->getParameterCount();
};

void unload(const void *app) {
  hide_gui(app);
  set_processing(app, false);

  auto vst = (PluginInstance *)app;

  vst->_vstPlug->setActive(false);

  if (vst->iConnectionPointComponent && vst->iConnectionPointController) {
    vst->iConnectionPointComponent->disconnect(vst->iConnectionPointController);
    vst->iConnectionPointController->disconnect(vst->iConnectionPointComponent);
  } else {
    std::cout << "Failed to get connection points." << std::endl;
  }

  vst->_editController->terminate();

  vst->_vstPlug->terminate();

  vst->destroy();

  // vst->_editController->release();
  // vst->_vstPlug->release();

  // delete vst;
};
