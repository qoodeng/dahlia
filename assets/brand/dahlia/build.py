"""Original flat copper Dahlia vector, rendered with the existing librsvg/iconutil tools."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent
# Rounded, cupped petals in staggered rings; no gradient, stroke, shadow or raster master.
PETAL = 'M 0 -31 C -13 -43 -20 -67 -10 -87 C -6 -98 6 -98 10 -87 C 20 -67 13 -43 0 -31 Z'
PALETTES = [('#B76B45', '#BD7650', '#A9603E', '#C17B54'), ('#CF8B61', '#C17A50', '#C78359', '#BC714B'), ('#A35F3D', '#B56D46', '#AC6440', '#BC7950')]

def flower(mono=False):
    paths=[]
    for ring, (count, scale, turn) in enumerate(((16, 1.05, -5), (12, .75, 10), (8, .46, -5))):
        for i in range(count):
            color='currentColor' if mono else PALETTES[ring][i%4]
            paths.append(f'<path fill="{color}" transform="translate(128 128) rotate({turn+i*360/count:g}) scale({scale:g})" d="{PETAL}"/>')
    paths.append('<path fill="'+('currentColor' if mono else '#925536')+'" d="M128 118 C134 118 138 122 138 128 C138 134 134 138 128 138 C122 138 118 134 118 128 C118 122 122 118 128 118Z"/>')
    return '\n'.join(paths)

def master(mono=False):
    return '<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256" color="#A66340" role="img" aria-labelledby="title desc"><title id="title">Dahlia</title><desc id="desc">A copper ball dahlia with rounded petals in three staggered rings.</desc>\n'+flower(mono)+'\n</svg>\n'

def render(src,dst,size=None):
    cmd=['rsvg-convert']
    if size:cmd+=['-w',str(size),'-h',str(size)]
    subprocess.run(cmd+['-o',str(dst),str(src)],check=True)

(ROOT/'dahlia-mark.svg').write_text(master())
(ROOT/'dahlia-mark-monochrome.svg').write_text(master(True))
for size in (16,32,64,128,256,512,1024):render(ROOT/'dahlia-mark.svg',ROOT/f'dahlia-mark-{size}.png',size)
for size in (16,32,64,256):render(ROOT/'dahlia-mark-monochrome.svg',ROOT/f'dahlia-monochrome-{size}.png',size)
parts=['<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="760" viewBox="0 0 1200 760">','<rect width="1200" height="760" fill="#F6F3EF"/>','<text x="72" y="84" font-family="Helvetica Neue,Helvetica,sans-serif" font-size="30" font-weight="500" fill="#38312D">Dahlia</text>','<text x="72" y="116" font-family="Helvetica Neue,Helvetica,sans-serif" font-size="15" fill="#82736A">Copper · original vector mark</text>',f'<svg x="80" y="180" width="430" height="430" viewBox="0 0 256 256">{flower()}</svg>',f'<svg x="725" y="228" width="256" height="256" viewBox="0 0 256 256" color="#A66340">{flower(True)}</svg>']
for x,size in ((675,16),(805,32),(960,64)):
    parts += [f'<image href="dahlia-mark-{size}.png" x="{x}" y="610" width="{size}" height="{size}"/>',f'<text x="{x+size/2}" y="710" text-anchor="middle" font-family="Helvetica Neue,Helvetica,sans-serif" font-size="12" fill="#82736A">{size} px</text>']
parts.append('</svg>');(ROOT/'dahlia-preview.svg').write_text('\n'.join(parts));render(ROOT/'dahlia-preview.svg',ROOT/'dahlia-preview.png')
# ICNS includes actual native raster sizes; all renders derive from the vector master.
iconset=ROOT/'Dahlia.iconset';iconset.mkdir(exist_ok=True)
for size in (16,32,128,256,512):
    render(ROOT/'dahlia-mark.svg',iconset/f'icon_{size}x{size}.png',size)
    render(ROOT/'dahlia-mark.svg',iconset/f'icon_{size}x{size}@2x.png',size*2)
subprocess.run(['iconutil','-c','icns',str(iconset),'-o',str(ROOT.parent/'dahlia.icns')],check=True)
