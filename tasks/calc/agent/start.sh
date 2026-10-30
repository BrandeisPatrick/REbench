#!/bin/bash
mkdir -p /workspace/submission
if [ "${CONDITION:-full}" != "no-gui" ]; then
  Xvfb :1 -screen 0 1280x800x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &
  for i in $(seq 50); do [ -e /tmp/.X11-unix/X1 ] && break; sleep 0.1; done
  DISPLAY=:1 openbox >/tmp/openbox.log 2>&1 &
  mkdir -p /root/.lo-gui && cp -r /opt/lo-profile-template/. /root/.lo-gui/
  sleep 1
  (cd /task/public/books && DISPLAY=:1 SAL_USE_VCLPLUGIN=gen soffice --calc --norestore --nologo \
     -env:UserInstallation=file:///root/.lo-gui /task/public/books/book_1_0000.xlsx >/tmp/soffice-gui.log 2>&1 &)
fi
exec sleep infinity
