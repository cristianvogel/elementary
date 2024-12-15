#include <algorithm>
#include <array>

#include "Bindings.h"


RuntimeBindings::RuntimeBindings(double sampleRate, size_t blockSize)
    : m_runtime(sampleRate, blockSize)
{
}

RuntimeBindings::~RuntimeBindings()
{
}

int RuntimeBindings::apply_instructions(rust::string const& batch)
{
    return m_runtime.applyInstructions(elem::js::parseJSON((std::string) batch));
}

void RuntimeBindings::process(const float* inputData, float* outputData, size_t numChannels, size_t numFrames)
{
    std::array<float*, 32> outChans;

    for (size_t i = 0; i < numChannels; ++i) {
        outChans[i] = outputData + (i * numFrames);
    }

    m_runtime.process(
        nullptr,
        0,
        outChans.data(),
        numChannels,
        numFrames,
        nullptr
    );
}

std::unique_ptr<RuntimeBindings> new_runtime_instance(double sampleRate, size_t blockSize) {
    return std::make_unique<RuntimeBindings>(sampleRate, blockSize);
}
