#!/usr/bin/env bash
# Fetch the pinned research-paper PDFs (read-only study material) and write
# PAPERS.json with url + sha256 per paper. PDFs land in papers/<topic>/ (git-ignored
# via references/*/); this script + PAPERS.json are the tracked, reproducible record.
# Idempotent: existing files are hashed, not re-downloaded. Paywalled works are
# recorded as cite_only entries so the citation set is complete.
set -u

cd "$(dirname "$0")"
mkdir -p papers/eye-tracking

# name|topic|url  (url = CITE_ONLY:<citation> for paywalled/cite-only entries)
papers=(
  'swirski-dodgson-2013-eye-model-fit|eye-tracking|https://www.cl.cam.ac.uk/research/rainbow/projects/eyemodelfit/files/Swirski,%20Dodgson%20-%202013%20-%20A%20fully-automatic,%20temporal%20approach%20to%20single%20camera,%20glint-free%203D%20eye%20model%20fitting.pdf'
  'santini-2018-pure|eye-tracking|https://arxiv.org/pdf/1712.08900'
  'fuhl-2016-else|eye-tracking|https://arxiv.org/pdf/1511.06575'
  'chaudhary-2019-ritnet|eye-tracking|https://arxiv.org/pdf/1910.00694'
  'kothari-2021-ellseg|eye-tracking|https://arxiv.org/pdf/2007.09600'
  'garbin-2019-openeds|eye-tracking|https://arxiv.org/pdf/1905.03702'
  'kim-2019-nvgaze|eye-tracking|https://users.aalto.fi/~laines9/publications/kim2019sigchi_paper.pdf'
  'guestrin-eizenman-2006-pccr|eye-tracking|CITE_ONLY:Guestrin & Eizenman 2006, "General theory of remote gaze estimation using the pupil center and corneal reflections", IEEE TBME 53(6). doi:10.1109/TBME.2005.863952'
  'dierkes-2018-refraction-eye-model|eye-tracking|CITE_ONLY:Dierkes, Kassner, Bulling 2018, "A novel approach to single camera, glint-free 3D eye model fitting including corneal refraction", ETRA 2018. doi:10.1145/3204493.3204525 (basis of pye3d; see references/pye3d clone)'
  'santini-2018-purest|eye-tracking|CITE_ONLY:Santini, Fuhl, Kasneci 2018, "PuReST: robust pupil tracking for real-time pervasive eye tracking", ETRA 2018. doi:10.1145/3204493.3204578'
  'fuhl-2015-excuse|eye-tracking|CITE_ONLY:Fuhl et al. 2015, "ExCuSe: Robust pupil detection in real-world scenarios", CAIP 2015. doi:10.1007/978-3-319-23192-1_4'
)

{
  echo '{'
  first=1
  for entry in "${papers[@]}"; do
    IFS='|' read -r name topic url <<<"$entry"
    [ $first -eq 1 ] || echo ','
    first=0
    case "$url" in
    CITE_ONLY:*)
      citation="${url#CITE_ONLY:}"
      printf '  "%s": {"topic": "%s", "status": "cite_only", "citation": "%s"}' \
        "$name" "$topic" "${citation//\"/\\\"}"
      ;;
    *)
      out="papers/$topic/$name.pdf"
      if [ ! -s "$out" ]; then
        curl --fail --location --retry 3 --max-time 120 -o "$out" "$url" \
          >/dev/null 2>&1 || rm -f "$out"
      fi
      if [ -s "$out" ] && head -c4 "$out" | grep -q '%PDF'; then
        sha=$(sha256sum "$out" | cut -d' ' -f1)
        printf '  "%s": {"topic": "%s", "status": "fetched", "url": "%s", "sha256": "%s"}' \
          "$name" "$topic" "$url" "$sha"
      else
        rm -f "$out"
        printf '  "%s": {"topic": "%s", "status": "fetch_failed", "url": "%s"}' \
          "$name" "$topic" "$url"
      fi
      ;;
    esac
  done
  echo
  echo '}'
} >PAPERS.json

echo "Papers manifest written: $(pwd)/PAPERS.json"
grep -c '"status": "fetched"' PAPERS.json | xargs echo "fetched:"
grep -c '"status": "fetch_failed"' PAPERS.json | xargs echo "failed:" || true
