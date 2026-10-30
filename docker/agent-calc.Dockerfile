# Agent sandbox for the calc task: toolchain + LibreOffice Calc (GUI and CLI) + virtual display.
FROM rb-toolchain:latest
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      libreoffice-calc python3-uno python3-openpyxl \
      xvfb openbox xdotool imagemagick x11-utils x11-xserver-utils dbus-x11 \
      fonts-liberation fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*
COPY tasks/calc/oracle/lo-profile /opt/lo-profile-template
COPY tasks/calc/agent /task
COPY grader/xlsxio.py /task/bin/xlsxio.py
COPY grader/compare.py /task/bin/compare.py
RUN chmod +x /task/bin/* /task/start.sh && mkdir -p /workspace/submission \
    && cp /opt/vendor/RUST_CRATES.txt /task/RUST_CRATES.txt \
    && echo 'export PATH=/task/bin:$PATH DISPLAY=:1' > /etc/profile.d/task.sh
ENV PATH=/task/bin:$PATH DISPLAY=:1
WORKDIR /workspace
CMD ["/task/start.sh"]
