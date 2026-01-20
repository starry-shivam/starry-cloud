# -------- BUILD STAGE --------
FROM python:3.12-alpine AS builder

WORKDIR /build

RUN pip install --no-cache-dir pyyaml jinja2

COPY config.yml .
COPY templates ./templates
COPY static ./static
COPY render.py .

RUN python render.py


# -------- RUNTIME STAGE --------
FROM busybox:uclibc

WORKDIR /www

COPY --from=builder /build/site /www

EXPOSE 5000

CMD ["httpd", "-f", "-p", "5000", "-h", "/www"]