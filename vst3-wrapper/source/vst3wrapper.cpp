#include "common.h"
#include "plugininstance.h"
#include "plugframe.h"

#include <cstdint>
#include <cstdio>
#include <iostream>

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

Steinberg::Vst::HostApplication *PluginInstance::standard_plugin_context =
    nullptr;
int PluginInstance::standard_plugin_context_ref_count = 0;

PluginInstance::PluginInstance() {}

PluginInstance::~PluginInstance() { destroy(); }

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

uint32_t get_latency(const void *app) {
  ffi_ensure_main_thread("[VST3] get_latency");

  PluginInstance *vst = (PluginInstance *)app;

  // https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical+Documentation/Workflow+Diagrams/Get+Latency+Call+Sequence.html

  // [(UI-thread or processing-thread) & Activated]
  vst->audio_processor->setProcessing(false);

  // [UI-thread & Setup Done]
  vst->component->setActive(false);

  // Gets and sends tail length changed update. This should eventually be done
  // somewhere else. [UI-thread & Setup Done]
  uint32_t tail = vst->audio_processor->getTailSamples();
  PluginIssuedEvent event = {};
  event.tag = PluginIssuedEvent::Tag::TailLengthChanged;
  event.tail_length_changed = {};
  event.tail_length_changed._0 = tail;
  send_event_to_host(&event, vst->rust_side_vst3_instance_object);

  vst->component->setActive(true);

  // [(UI-thread or processing-thread) & Activated]
  uint32_t latency = vst->audio_processor->getLatencySamples();

  // [(UI-thread or processing-thread) & Activated]
  vst->audio_processor->setProcessing(true);
  return latency;
}

void set_processing(const void *app, bool processing) {
  // TODO: Ensure activated

  PluginInstance *vst = (PluginInstance *)app;

  // [(UI-thread or processing-thread) & Activated]
  vst->audio_processor->setProcessing(processing);
}

Steinberg::Vst::ProcessContext *PluginInstance::processContext() {
  return &_processContext;
}

Steinberg::Vst::EventList *
PluginInstance::eventList(Steinberg::Vst::BusDirection direction, int which) {
  if (direction == kInput) {
    return static_cast<Steinberg::Vst::EventList *>(
        &process_data.inputEvents[which]);
  } else if (direction == kOutput) {
    return static_cast<Steinberg::Vst::EventList *>(
        &process_data.outputEvents[which]);
  } else {
    return nullptr;
  }
}

Steinberg::Vst::ParameterChanges *
PluginInstance::parameterChanges(Steinberg::Vst::BusDirection direction,
                                 int which) {
  if (direction == kInput) {
    return static_cast<Steinberg::Vst::ParameterChanges *>(
        &process_data.inputParameterChanges[which]);
  } else if (direction == kOutput) {
    return static_cast<Steinberg::Vst::ParameterChanges *>(
        &process_data.outputParameterChanges[which]);
  } else {
    return nullptr;
  }
}

IOConfigutaion PluginInstance::get_io_config() {
  IOConfigutaion io_config = {};
  io_config.audio_inputs = {};
  io_config.audio_outputs = {};

  auto audio_inputs =
      component->getBusCount(MediaTypes::kAudio, BusDirections::kInput);
  auto audio_outputs =
      component->getBusCount(MediaTypes::kAudio, BusDirections::kOutput);
  auto event_inputs =
      component->getBusCount(MediaTypes::kEvent, BusDirections::kInput);

  for (int i = 0; i < audio_inputs; i++) {
    BusInfo info;
    component->getBusInfo(MediaTypes::kAudio, BusDirections::kInput, i, info);
    io_config.audio_inputs.count++;
    io_config.audio_inputs.data[i] = {};
    io_config.audio_inputs.data[i].value.channels = info.channelCount;
  }

  for (int i = 0; i < audio_outputs; i++) {
    BusInfo info;
    component->getBusInfo(MediaTypes::kAudio, BusDirections::kOutput, i, info);
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

const void *load_plugin(const char *s, const char *id,
                        const void *rust_side_vst3_instance_object) {
  PluginInstance *vst = new PluginInstance();
  vst->rust_side_vst3_instance_object = rust_side_vst3_instance_object;
  vst->init(s, id);

  auto aud_in = vst->component->getBusCount(kAudio, kInput);
  for (int i = 0; i < aud_in; i++) {
    vst->component->activateBus(kAudio, kInput, i, true);
  }

  auto aud_out = vst->component->getBusCount(kAudio, kOutput);
  for (int i = 0; i < aud_out; i++) {
    vst->component->activateBus(kAudio, kOutput, i, true);
  }

  auto evt_in = vst->component->getBusCount(kEvent, kInput);
  for (int i = 0; i < evt_in; i++) {
    vst->component->activateBus(kEvent, kInput, i, true);
  }

  if (vst->component->setActive(true) != kResultTrue) {
    std::cout << "Failed to activate VST component" << std::endl;
  }

  if (vst->audio_processor->setProcessing(true)) {
    std::cout << "Failed to being processing" << std::endl;
  }

  // NOTE: Output event buses are not supported yet so they are not activated

  return vst;
}

Dims show_gui(const void *app, const void *window_id,
              WindowIDType window_id_type) {
  PluginInstance *vst = (PluginInstance *)app;

  if (!vst->edit_controller) {
    std::cerr << "VST does not provide an edit controller" << std::endl;
    return {};
  }

  if (!vst->_view) {
    vst->_view = vst->edit_controller->createView(ViewType::kEditor);
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
  case WindowIDType::XWNDWayland:
  case WindowIDType::Other:
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
  vst->audio_processor->setProcessing(false);

  // [UI-thread & Setup Done]
  vst->component->setActive(false);

  vst->process_setup.sampleRate = rate;

  // [UI-thread & (Initialized | Connected)]]
  vst->audio_processor->setupProcessing(vst->process_setup);

  // [UI-thread & Setup Done]
  vst->component->setActive(true);

  // [(UI-thread or processing-thread) & Activated]
  vst->audio_processor->setProcessing(true);

  vst->process_data.processContext->sampleRate = rate;
}

const void *get_data(const void *app, int32_t *data_len, const void **stream) {
  PluginInstance *vst = (PluginInstance *)app;

  ResizableMemoryIBStream *stream_ = new ResizableMemoryIBStream();
  *stream = stream_;

  if (vst->component->getState(stream_) != kResultOk) {
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
  if (vst->edit_controller->getState(stream_) != kResultOk) {
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
  if (vst->component->setState(&stream) != kResultOk) {
    std::cerr << "Failed to set processor state" << std::endl;
  }

  stream.rewind();

  // [UI-thread & Connected]
  if (vst->edit_controller->setComponentState(&stream) != kResultOk) {
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
  if (vst->edit_controller->setState(&stream) != kResultOk) {
    std::cout << "Failed to set controller state" << std::endl;
  }
}

void process(const void *app, const ProcessDetails *data, float ***input,
             float ***output, HostIssuedEvent *events, int32_t events_len) {
  ffi_ensure_non_main_thread("[VST3] process");
  PluginInstance *vst = (PluginInstance *)app;

  auto audio_inputs = vst->_io_config.audio_inputs.count;
  auto audio_outputs = vst->_io_config.audio_outputs.count;

  vst->process_data.numSamples = data->block_size;

  for (int i = 0; i < audio_inputs; i++) {
    vst->process_data.inputs[i].numChannels =
        vst->_io_config.audio_inputs.data[i].value.channels;
    vst->process_data.inputs[i].silenceFlags = 0;
    vst->process_data.inputs[i].channelBuffers32 = input[i];
  }

  vst->process_data.numInputs = audio_inputs;

  for (int i = 0; i < audio_outputs; i++) {
    vst->process_data.outputs[i].numChannels =
        vst->_io_config.audio_outputs.data[i].value.channels;
    vst->process_data.outputs[i].silenceFlags = 0;
    vst->process_data.outputs[i].channelBuffers32 = output[i];
  }

  vst->process_data.numOutputs = audio_outputs;

  Steinberg::uint32 state = 0;

  Steinberg::Vst::ProcessContext *ctx = vst->process_data.processContext;

  vst->process_data.processContext->tempo = data->tempo;
  state |= ctx->kTempoValid;

  vst->process_data.processContext->timeSigNumerator =
      data->time_signature_numerator;
  vst->process_data.processContext->timeSigDenominator =
      data->time_signature_denominator;
  state |= ctx->kTimeSigValid;

  vst->process_data.processContext->projectTimeMusic = data->player_time;

  vst->process_data.processContext->projectTimeSamples =
      (data->player_time / (data->tempo / 60.)) * data->sample_rate;

  // TODO
  // vst->_processData.processContext->barPositionMusic = data.barPosBeats;
  // state |= ctx->kBarPositionValid;

  vst->process_data.processContext->cycleStartMusic = data->cycle_start;
  vst->process_data.processContext->cycleEndMusic = data->cycle_end;
  state |= ctx->kCycleValid;

  vst->process_data.processContext->systemTime = data->nanos;
  state |= ctx->kSystemTimeValid;

  vst->process_data.processContext->frameRate.framesPerSecond = 60.;
  vst->process_data.processContext->frameRate.flags = 0;

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
    vst->process_data.processMode = kOffline;
  } else {
    vst->process_data.processMode = kRealtime;
  }

  vst->process_data.processContext->state = state;

  int midi_bus = 0;
  Steinberg::Vst::EventList *eventList = nullptr;

  if (!vst->process_data.inputParameterChanges) {
    vst->process_data.inputParameterChanges = new ParameterChanges(400);
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

            auto changes = vst->process_data.inputParameterChanges;

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

    auto changes = vst->process_data.inputParameterChanges;

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
  tresult result = vst->audio_processor->process(vst->process_data);
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
  vst->edit_controller->queryInterface(IInfoListener::iid,
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
  if (vst->edit_controller->setParamNormalized(id, value) != kResultOk) {
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
  vst->edit_controller->getParameterInfo(index, param_info);

  vst->component_handler->parameter_indicies[param_info.id] = index;

  // TODO: Make real-time safe with stack buffers

  std::string name = {};
  for (TChar c : param_info.title) {
    if (c != '\0') {
      name += c;
    }
  }

  Steinberg::Vst::ParamValue value =
      vst->edit_controller->getParamNormalized(param_info.id);

  TChar formatted_value[128] = {};
  if (vst->edit_controller->getParamStringByValue(
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
  return vst->edit_controller->getParameterCount();
};

void unload(const void *app) {
  hide_gui(app);
  set_processing(app, false);

  auto vst = (PluginInstance *)app;

  vst->component->setActive(false);

  if (vst->iConnectionPointComponent && vst->iConnectionPointController) {
    vst->iConnectionPointComponent->disconnect(vst->iConnectionPointController);
    vst->iConnectionPointController->disconnect(vst->iConnectionPointComponent);
  } else {
    std::cout << "Failed to get connection points." << std::endl;
  }

  vst->edit_controller->terminate();

  vst->component->terminate();

  vst->destroy();

  // vst->_editController->release();
  // vst->_vstPlug->release();

  // delete vst;
};
