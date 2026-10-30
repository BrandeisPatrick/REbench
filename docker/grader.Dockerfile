# Grader: the toolchain image plus grading scripts. No LibreOffice, no spreadsheet libraries.
# Submissions build and run as the unprivileged user "runner"; hidden truth lives under /root (mode 700).
FROM rb-toolchain:latest
RUN useradd -m -u 1000 runner && chmod -R a+rwX /opt/cargo && chmod 700 /root
COPY grader /grader
WORKDIR /grader
