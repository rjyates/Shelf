FROM qemux/qemu:7.50@sha256:e7f6fda52503a546fd649670ba46e4bc23dc6dcef275bc3fac48877fbbc430df

# After boot, qemux removes /run/shm/index.html and nginx falls through to
# @vnc → /usr/share/novnc/vnc.html. Put the e-ink viewer there.
COPY viewer/index.html /usr/share/novnc/vnc.html
COPY viewer/boot.js viewer/panel.js viewer/viewer.css /usr/share/novnc/
