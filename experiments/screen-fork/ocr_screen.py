#!/usr/bin/env python3
"""Screen-content Q&A via OCR + a small local text model (no vision model).

Pipeline: screenshot file -> preprocess (2x upscale, grayscale, autocontrast)
-> tesseract -> OCR text -> llama-server chat completions (text-only model)
-> answer grounded ONLY in the OCR text.

Measured on a synthetic 1920x1080 desktop (sandbox, 1-thread VM):
OCR ~1 s, answer ~9 s on qwen2.5-0.5b, correct window list + content, no
hallucination on text questions. Fails (guesses) on visual/spatial/color
questions - OCR carries no pixels. See docs/EXPERIMENTS.md 4e.

Free-only: tesseract (Apache-2.0), any local OpenAI-compatible server.
"""
import base64, json, subprocess, sys, tempfile, urllib.request, os

def preprocess(src, dst):
    from PIL import Image, ImageOps
    img = Image.open(src).convert("L")
    w, h = img.size
    img = img.resize((w * 2, h * 2), Image.LANCZOS)
    ImageOps.autocontrast(img).save(dst)

def ocr(png):
    out = subprocess.run(["tesseract", png, "stdout", "--psm", "3"],
                         capture_output=True, text=True, check=True)
    return out.stdout

def ask(server, model_note, ocr_text, question):
    body = {"messages": [
        {"role": "system", "content":
         "You answer questions about the user's screen using ONLY the OCR "
         "text provided. If the answer is not in the OCR text, say that the "
         "OCR text does not show it. Do not invent anything."},
        {"role": "user", "content":
         "OCR text from my screen:\n" + ocr_text + "\n\nQuestion: " + question}],
        "max_tokens": 300, "temperature": 0.0}
    req = urllib.request.Request(server + "/v1/chat/completions",
        data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    r = json.load(urllib.request.urlopen(req, timeout=300))
    return r["choices"][0]["message"]["content"], r.get("usage")

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("usage: ocr_screen.py SCREENSHOT 'question' [SERVER_URL]")
        sys.exit(2)
    shot, question = sys.argv[1], sys.argv[2]
    server = sys.argv[3] if len(sys.argv) > 3 else "http://127.0.0.1:8080"
    with tempfile.TemporaryDirectory() as td:
        prep = os.path.join(td, "prep.png")
        preprocess(shot, prep)
        text = ocr(prep)
    answer, usage = ask(server, "", text, question)
    print(answer)
    print("---", usage, file=sys.stderr)
