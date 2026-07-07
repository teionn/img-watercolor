from pathlib import Path
import cv2
from skimage import data

Path("input").mkdir(exist_ok=True)
for name, img in [("cat", data.chelsea()), ("coffee", data.coffee()), ("astronaut", data.astronaut())]:
    cv2.imwrite(f"input/{name}.png", cv2.cvtColor(img, cv2.COLOR_RGB2BGR))
    print(name, img.shape)
