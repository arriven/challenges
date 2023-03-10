# Imagine you were asked to create an ad serving system (a simplified clone of Google AdSense) for the UK market

- describe the high-level architecture of the project.
- highlight key components / micro services.
- estimate RPS if your banners will be placed on the top 10 sites in the UK (one banner per page).
- how do you organize the advertising targeting system?
- what technologies will you offer?
- how will you track statistics?
- where will you store statistics on impressions / clicks?
- estimate the approximate Total Cost of Ownership.

## Solution

I'll start with estimating the RPS of the service since that will impact the architecture and cost of the project. According to [semrush.com](https://www.semrush.com/website/top/united-kingdom/all/) the monthly traffic to top 10 UK websites is 9,162,151,786 visits based on december 2022. That averages to 3420 visits per second but due to the nature of the traffic we should expect different numbers at peak vs off-peak hours. Considering that the sites in the list are mostly in the entertainment/news categories and that most of the visitors reside in the same timezone it would be wise to assume that night and working hours are off-peak, meaning that majority of the traffic is handled during less than 30% of the day, so our system should be able to steadily handle 10000+ RPS based on this assumption. Even this kind of traffic is still non-linear so we should also account for short term spikes.

The high level architecture of the system would include some control panel for publishers, financial services, as serving subsystem, targeting system, and statistics.

The control panel and financial services would experience much lower general load (publishers population << users population) compared to other subsystems so there's not much need to cover them in the design too much. Financial services require higher reliability than the rest of the system but are also quite likely to be outsourced to some third party.

Ad service system should include some object storage for actual ads content and a CDN to optimize delivery speed and cost for different parts of the world. Since the content is changed relatively infrequently the cache refresh rate of a CDN shouldn't be of any issue and any decent CDN would suffice. The same system would most likely be reused for other static assets (i.e. client-side js).

I'm assuming that we can do targeting based on criteria that can be retrieved on the client side via browser APIs and cookies (so no additional backend is needed for tracking these criteria). This will be the most latency-critical system among the user-facing ones and to accomodate that I'd try to use some serverless computation with an access to either a distriuted nosql db with targeting algorithm parameters (is updated asynchronously by other services) or/and an ML model (behind a caching proxy to not overload the cost) that is fed by the statistics service. Nosql db can also be hidden behind a caching proxy to reduce cost but it's not as critical there and we should consider tradeoffs between cost and responsiveness of the system.

Statistics service doesn't have high latency requirements so I'd use a regular autoscaling compute (it's usually cheaper than serverless for the same amount of work and you have options to offload some work to spot/preemptive instances which are even cheaper) that would have a managed queue in front of it to handle spikes in load and also allow us a more flexible and composable processing pipeline (i.e. we can use spot/preemptible pools and we can have different workers for storing the data into data warehouse for long-term statistics and some OLTP storage for realtime ones). We would probably need some basic webserver in front of the queue to handle client requests and put them into a queue unless it's already provided by the vendor.

The approximate list of technologies for the general-user-facing systems and their cost estimation if we are to host on GCP

- Google Cloud CDN - $0.02-$0.20 per GiB for cache egress, $0.0075 per 10,000 requests for cache lookup. If we extimate each ad to be a resource of up to 100KiB on average it should lead to ~$7000/month in cache lookup and ~$18000/month in cache egress.
- Google Cloud Storage - $0.020 per GB per month of standard storage + $0.005 per 1000 insert operations + $0.0004 per 1000 read operations. If we are to route the ad serving directly to GCS it would only cost ~$4000 a month for requests. Considering that we only need to cover one region it could be a more viable option than a CDN.
- Google Cloud funtions to power the targeting system - $0.4 per million invocations + $0.000000231 per 100ms of cheapest compute unit. Provided that our targeting is mostly shuffling the data received from other sources it should be safe to assume that .9 percentile of requests would complete in under 500ms leading to overall cost of $1.15 per million invocations. With 9,162,151,786 visits per month it ends up costing us ~$10500/month
- Firestore for targeting data storage (at least list of ads it can choose from) - $0.06 per 100000 document reads. Assuming each targeting request has to read 1 document (unlikely but hey, maybe we have a static configuration) it would lead to ~$5500/month. This doesn't include updates done by statistics/console and the required storage and scales with the complexity of our targeting system.
- Vertex AI if we are to use ML in the targeting system - $5.00 per 1,000 text records, but we're definitely going to put a cache in front of it, ideally we should be able to tune it to cost under $1000/month (not including training).
- Google Pub/Sub - $40 per TiB if we don't have backlog and don't retain acked messages. Assuming each impression carries about ~10KiB of data and we only need 1 topic (all subscribers are independent) it would cost ~$80/month.
- Google cloud compute - $906.2512/month for a single e2-standard-32 node. Assuming each of our tasks takes 100ms of cpu time (and we are CPU bound) one node would be able to handle 320 RPS. With average of ~3500 RPS we would need ~11 machines like that per type of task leading to ~$10000/month. Offloading half of this work to spot instances would lead to at least 25% discount in general.
- Bigquery - $5 per TB of analyzed data + $0.02 per GB of active logical storage being ingested. Provided we use a fixed set of aggregations over the data we could utilize materialized views that are computed only when data gets into the system we would essentially only need to pay for the storage. Based on previous assumption of each impression carrying ~10Kib of data the cost would end up being ~$1500/month.

The overall cost ends up ranging from ~$32000 to ~$60000 per month. This doesn't include development/operational work. At this scale, choosing managed services makes more sense since properly operationalizing such a system with on-prem services would impose much higher development cost than the potential savings from such a move.
