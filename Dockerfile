FROM python:3.14.5-alpine

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY gen_auth.py .
COPY templates ./templates
COPY static ./static
COPY app/ ./app/

EXPOSE 5000

CMD gunicorn \
	--bind 0.0.0.0:5000 \
	--workers ${GUNICORN_WORKERS:-1} \
	--threads ${GUNICORN_THREADS:-2} \
	--worker-class gthread \
	--access-logfile - \
	--access-logformat 'ip=%(h)s xff=%({x-forwarded-for}i)s method=%(m)s path=%(U)s status=%(s)s rt=%(L)s' \
	--error-logfile - \
	--log-level info \
	--capture-output \
	app:app
