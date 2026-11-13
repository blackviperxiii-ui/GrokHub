// Push to talk: copies the mic's samples off the audio thread in batches of
// about 40 ms, so the page gets them without the deprecated script processor.
class MicTap extends AudioWorkletProcessor {
  constructor() {
    super();
    this.batch = [];
    this.size = 0;
  }

  process(inputs) {
    const channel = inputs[0] && inputs[0][0];
    if (channel) {
      this.batch.push(channel.slice(0));
      this.size += channel.length;
      if (this.size >= 2048) {
        const out = new Float32Array(this.size);
        let at = 0;
        for (const c of this.batch) {
          out.set(c, at);
          at += c.length;
        }
        this.port.postMessage(out, [out.buffer]);
        this.batch = [];
        this.size = 0;
      }
    }
    return true;
  }
}

registerProcessor("mic-tap", MicTap);
