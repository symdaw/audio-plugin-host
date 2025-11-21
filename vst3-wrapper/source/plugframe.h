#pragma once

#include "common.h"

class PlugFrame : public Steinberg::IPlugFrame {
public:
  const void *rust_side_vst3_instance_object = nullptr;

  PlugFrame(const void *_rust_side_vst3_instance_object);

  Steinberg::tresult resizeView(Steinberg::IPlugView *view,
                                Steinberg::ViewRect *newSize) override;

  Steinberg::tresult queryInterface(const Steinberg::TUID /*_iid*/,
                                    void ** /*obj*/) override;
  Steinberg::uint32 addRef() override;
  Steinberg::uint32 release() override;
};
