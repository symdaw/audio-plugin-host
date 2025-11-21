#pragma once

#include "common.h"

#include <mutex>
#include <unordered_map>

class ComponentHandler : public Steinberg::Vst::IComponentHandler {
public:
  ComponentHandler();

  Steinberg::tresult beginEdit(Steinberg::Vst::ParamID id) override;

  Steinberg::tresult
  performEdit(Steinberg::Vst::ParamID id,
              Steinberg::Vst::ParamValue valueNormalized) override;
  Steinberg::tresult endEdit(Steinberg::Vst::ParamID id) override;
  Steinberg::tresult restartComponent(Steinberg::int32 flags) override;

  inline ComponentHandler(const void *_rust_side_vst3_instance_object) {
    rust_side_vst3_instance_object = _rust_side_vst3_instance_object;

    param_edits = {};
    parameter_indicies = {};
  }

  std::unordered_map<Steinberg::Vst::ParamID, int> parameter_indicies;

private:
  const void *rust_side_vst3_instance_object = nullptr;
  std::vector<ParameterEditState> param_edits;
  std::mutex param_edits_mutex;

  Steinberg::tresult queryInterface(const Steinberg::TUID /*_iid*/,
                                    void ** /*obj*/) override;
  // we do not care here of the ref-counting. A plug-in call of release should
  // not destroy this class!
  Steinberg::uint32 addRef() override;
  Steinberg::uint32 release() override;

  void send_param_change_event(int32_t id, float value, float initial_value,
                               bool end_edit = false);
};
