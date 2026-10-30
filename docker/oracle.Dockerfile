# Oracle image: maintainer-only. Generates workbooks and ground truth with LibreOffice.
FROM debian:trixie-slim
ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 LC_ALL=C.UTF-8
RUN apt-get update && apt-get install -y --no-install-recommends \
      libreoffice-calc-nogui python3-uno python3 python3-openpyxl fonts-liberation procps \
    && rm -rf /var/lib/apt/lists/*
COPY tasks/calc/oracle/lo-profile /opt/lo-profile-template
COPY tasks/calc/oracle /oracle
COPY grader/xlsxio.py /oracle/xlsxio.py
