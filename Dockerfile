FROM python:3.12-alpine

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY config.yml .
COPY templates ./templates
COPY static ./static
COPY app/ ./app/

EXPOSE 5000

CMD ["sh", "-c", "gunicorn --bind 0.0.0.0:5000 --workers ${GUNICORN_WORKERS:-1} --threads ${GUNICORN_THREADS:-4} --worker-class gthread app:app"]
