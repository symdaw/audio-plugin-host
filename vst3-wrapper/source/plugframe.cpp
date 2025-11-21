#include "plugframe.h"

PlugFrame::PlugFrame(const void *_rust_side_vst3_instance_object) {
  rust_side_vst3_instance_object = _rust_side_vst3_instance_object;
}

Steinberg::tresult PlugFrame::resizeView(Steinberg::IPlugView *view,
                                Steinberg::ViewRect *newSize) {
  PluginIssuedEvent event = {};
  event.tag = PluginIssuedEvent::Tag::ResizeWindow;
  event.resize_window = {};
  event.resize_window._0 = (uintptr_t)newSize->getWidth();
  event.resize_window._1 = (uintptr_t)newSize->getHeight();

  send_event_to_host(&event, rust_side_vst3_instance_object);

  return Steinberg::kResultOk;
}

Steinberg::tresult PlugFrame::queryInterface(const Steinberg::TUID /*_iid*/,
                                  void ** /*obj*/) {
  return Steinberg::kNoInterface;
}

// we do not care here of the ref-counting. A plug-in call of release should
// not destroy this class!
Steinberg::uint32 PlugFrame::addRef() { return 1000; }
Steinberg::uint32 PlugFrame::release() { return 1000; }
