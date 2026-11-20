#!/bin/sh
# Simulates recordings with macOS text-to-speech: one 16 kHz mono WAV per
# sentence in sentences/<set>.txt, written to audio/<set>-NN.wav (gitignored).
# Usage: spikes/voice/synthesize.sh
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$here/audio"

synth() {
  set_name="$1"
  voice="$2"
  n=0
  while IFS= read -r line || [ -n "$line" ]; do
    [ -z "$line" ] && continue
    n=$((n + 1))
    id="$(printf '%02d' "$n")"
    say -v "$voice" -o "$tmp/clip.aiff" "$line"
    afconvert -f WAVE -d LEI16@16000 -c 1 "$tmp/clip.aiff" "$here/audio/$set_name-$id.wav"
  done <"$here/sentences/$set_name.txt"
  echo "$set_name: $n clips (voice: $voice)"
}

synth vi Linh
synth en Samantha
# Mixed sentences are Vietnamese with English technical terms; the Vietnamese
# voice reads the English words with a Vietnamese accent, like a local speaker.
synth mixed Linh
